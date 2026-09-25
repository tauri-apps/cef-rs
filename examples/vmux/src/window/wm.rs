//! Apply tiling / stack layout to winit shells in `windows_store`.
//!
//! Chooses layout paths from settings (tile vs stack, grid vs dwindle vs BSP). **BSP / manual tmux**
//! pane-tree work lives in [`crate::tmux::layout`] and [`crate::tmux::pane_tree`].
//!
//! Tmux-style **leader** and global WM chords (`[window_manager.tmux]`) run in [`crate::tmux::keyboard`]
//! when [`crate::settings::WmMode::Manual`]; generic title HUD helpers live here.
//!
//! ## Debug leader / splits
//! - Window title shows a **` · [WM …]`** suffix while the leader is armed or after a command.
//! - Logs: `RUST_LOG=vmux_wm=debug` (or `info` for high-signal lines only).
//! - If **`%`** / **`"`** still fails: check logs for `mods_shift`, `text`, `logical_key`; leader stays armed if
//!   the second key arrives while `editable_focus_is_typing` (focus a non-field or press `i`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::window::{Window, WindowId};

use cef::{ImplBrowser, ImplBrowserHost, PaintElementType};

use crate::browser::cef as vmux_cef;
use crate::browser::cef::osr::WindowEntry;
use crate::browser::cef::shell::OsrHostState;
use crate::runtime::{RuntimeState, WmAction};
use crate::settings::{KeySettings, WmGeometryLayout, WmTileStrategy};

use super::tiling::{
    PhysRect, dwindle_tile_rects, grid_tile_rects,
};

// --- Window-manager HUD (suffix on winit titles while leader armed or after a command) ---

/// Suffix marker; stripped by [`strip_wm_hud_from_title`] before appending a new HUD.
pub const WM_HUD_TITLE_INFIX: &str = " · [WM ";

pub fn strip_wm_hud_from_title(title: &str) -> String {
    title
        .split(WM_HUD_TITLE_INFIX)
        .next()
        .unwrap_or(title)
        .to_string()
}

fn format_with_hud(base: &str, line: &str) -> String {
    let b = strip_wm_hud_from_title(base);
    format!("{b}{WM_HUD_TITLE_INFIX}{line}]")
}

/// Clear HUD when past deadline; call before applying titles.
pub fn wm_hud_tick(rt: &mut RuntimeState) {
    if rt.wm_hud_message.is_empty() {
        rt.wm_hud_clear_deadline = None;
        return;
    }
    if let Some(deadline) = rt.wm_hud_clear_deadline {
        if Instant::now() >= deadline {
            rt.wm_hud_message.clear();
            rt.wm_hud_clear_deadline = None;
        }
    }
}

pub fn wm_hud_set(rt: &mut RuntimeState, line: impl Into<String>, visible_for: Duration) {
    rt.wm_hud_message = line.into();
    rt.wm_hud_clear_deadline = Some(Instant::now() + visible_for);
}

pub fn wm_hud_clear(rt: &mut RuntimeState) {
    rt.wm_hud_message.clear();
    rt.wm_hud_clear_deadline = None;
}

fn apply_hud_to_window(win: &Window, line: Option<&str>) {
    let cur = win.title();
    let base = strip_wm_hud_from_title(&cur);
    match line {
        None | Some("") => {
            if cur != base {
                let _ = win.set_title(&base);
            }
        }
        Some(h) => {
            let t = format_with_hud(&base, h);
            if cur != t {
                let _ = win.set_title(&t);
            }
        }
    }
}

/// Sync every OSR shell title with [`RuntimeState::wm_hud_message`].
pub fn apply_wm_hud_to_all_shell_windows(osr_host: &OsrHostState, rt: &mut RuntimeState) {
    wm_hud_tick(rt);
    let Ok(guard) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };
    let line = (!rt.wm_hud_message.is_empty()).then_some(rt.wm_hud_message.as_str());
    for (_, entry) in guard.iter() {
        apply_hud_to_window(&entry.surface.window, line);
    }
}

fn monitor_work_phys(win: &winit::window::Window) -> PhysRect {
    if let Some(m) = win.current_monitor() {
        let p = m.position();
        let s = m.size();
        PhysRect {
            x: p.x,
            y: p.y,
            w: s.width,
            h: s.height,
        }
    } else {
        let s = win.inner_size();
        let o = win.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
        PhysRect {
            x: o.x,
            y: o.y,
            w: s.width,
            h: s.height,
        }
    }
}

fn apply_rect(win: &winit::window::Window, r: PhysRect) {
    let _ = win.set_outer_position(PhysicalPosition::new(r.x, r.y));
    let _ = win.request_inner_size(PhysicalSize::new(r.w.max(1), r.h.max(1)));
}

/// After [`apply_rect`], winit may not emit `Resized` before the next frame; CEF + wgpu still need
/// the same updates as [`crate::window::dispatch`]'s `WindowEvent::Resized` path (fixes letterboxed /
/// stretched OSR after tile/stack toggles and WM moves).
fn sync_osr_shells_after_wm_move(guard: &mut HashMap<WindowId, WindowEntry>) {
    let gpu = &*crate::runtime::ffi_gpu();
    for entry in guard.values_mut() {
        let physical = entry.surface.window.inner_size();
        if physical.width == 0 || physical.height == 0 {
            continue;
        }
        entry.surface.resize(gpu, physical);
        vmux_cef::set_device_scale_factor(entry.surface.window.scale_factor() as f32);
        let logical = physical.to_logical(entry.surface.window.scale_factor());
        if let Ok(mut s) = entry.size.lock() {
            *s = logical;
        }
        if let Some(host) = entry.browser.host() {
            host.notify_screen_info_changed();
            host.was_resized();
            host.invalidate(PaintElementType::default());
        }
        entry.surface.window.request_redraw();
    }
}

/// Tile or stack all OSR shells according to [`RuntimeState::wm_geometry_layout`] and settings.
pub fn apply_window_layout(osr_host: &OsrHostState, rt: &mut RuntimeState) {
    let Ok(mut guard) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };
    apply_window_layout_locked(&*osr_host.key_settings, rt, &mut guard);
}

pub(crate) fn apply_window_layout_locked(
    ks: &KeySettings,
    rt: &mut RuntimeState,
    guard: &mut std::collections::HashMap<WindowId, WindowEntry>,
) {
    use std::collections::HashSet;

    let mut pairs: Vec<(WindowId, &WindowEntry)> = guard.iter().map(|(a, b)| (*a, b)).collect();
    pairs.sort_by_key(|(_, e)| e.browser.identifier());
    if pairs.is_empty() {
        return;
    }
    let work = monitor_work_phys(&pairs[0].1.surface.window);
    let n = pairs.len();

    if ks.wm_balance_on_resize && (work.w != rt.wm_last_work_w || work.h != rt.wm_last_work_h) {
        rt.wm_last_work_w = work.w;
        rt.wm_last_work_h = work.h;
        rt.wm_dwindle_axis_phase = if ks.wm_default_split_axis_vertical {
            0
        } else {
            1
        };
    }

    if rt.wm_geometry_layout == WmGeometryLayout::Tile
        && ks.effective_tile_strategy() == WmTileStrategy::Bsp
    {
        let valid: HashSet<WindowId> = pairs.iter().map(|(w, _)| *w).collect();
        let wids_sorted: Vec<WindowId> = pairs.iter().map(|(w, _)| *w).collect();

        if let Some(map) =
            crate::tmux::layout::bsp_pane_rects_or_fallback(rt, &valid, &wids_sorted, work)
        {
            for (wid, entry) in &pairs {
                let r = map.get(wid).copied().unwrap_or(work);
                apply_rect(&entry.surface.window, r);
            }
            drop(pairs);
            sync_osr_shells_after_wm_move(guard);
            return;
        }
    }

    let rects: Vec<PhysRect> = match rt.wm_geometry_layout {
        WmGeometryLayout::Stack => vec![work; n],
        WmGeometryLayout::Tile => {
            let strat = ks.effective_tile_strategy();
            match strat {
                WmTileStrategy::Grid => grid_tile_rects(work, n),
                WmTileStrategy::Dwindle | WmTileStrategy::Bsp => {
                    let phase = rt.wm_dwindle_axis_phase;
                    dwindle_tile_rects(work, n, phase)
                }
            }
        }
    };

    if rects.len() != n {
        return;
    }

    for ((_wid, entry), r) in pairs.iter().zip(rects.iter()) {
        apply_rect(&entry.surface.window, *r);
    }

    let stack_focus_window = if rt.wm_geometry_layout == WmGeometryLayout::Stack {
        rt.wm_focused_shell_window.and_then(|fw| {
            pairs
                .iter()
                .find(|(id, _)| *id == fw)
                .map(|(_, e)| e.surface.window.clone())
        })
    } else {
        None
    };
    drop(pairs);
    sync_osr_shells_after_wm_move(guard);
    if let Some(w) = stack_focus_window {
        w.focus_window();
    }
}

/// Cycle shell focus in stable browser-id order (for stack mode and multi-window UX).
pub fn focus_shell_relative(osr_host: &OsrHostState, rt: &mut RuntimeState, delta: isize) {
    let Ok(guard) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };
    let mut ids: Vec<WindowId> = guard.keys().copied().collect();
    if ids.len() < 2 {
        return;
    }
    ids.sort_by_key(|wid| guard.get(wid).map(|e| e.browser.identifier()).unwrap_or(0));
    let cur = rt
        .wm_focused_shell_window
        .and_then(|f| ids.iter().position(|w| *w == f))
        .unwrap_or(0);
    let n = ids.len() as isize;
    let next = (cur as isize + delta).rem_euclid(n) as usize;
    let wid = ids[next];
    if let Some(e) = guard.get(&wid) {
        e.surface.window.focus_window();
    }
    rt.wm_focused_shell_window = Some(wid);
    drop(guard);
    if let Some(bid) = osr_host
        .cef_attach
        .windows_store
        .lock()
        .ok()
        .and_then(|g| g.get(&wid).map(|e| e.browser.identifier()))
    {
        osr_host.request_set_active_browser(bid);
    }
}

/// Handle manual WM actions (keybindings → state → layout).
pub fn handle_wm_action(osr_host: &OsrHostState, rt: &mut RuntimeState, action: WmAction) {
    let ks = &*osr_host.key_settings;
    match action {
        WmAction::ToggleLayout => {
            rt.wm_geometry_layout = rt.wm_geometry_layout.toggle();
        }
        WmAction::FocusNext => {
            focus_shell_relative(osr_host, rt, 1);
            apply_window_layout(osr_host, rt);
            return;
        }
        WmAction::FocusPrev => {
            focus_shell_relative(osr_host, rt, -1);
            apply_window_layout(osr_host, rt);
            return;
        }
        WmAction::SplitSideBySide | WmAction::SplitStacked => {}
        WmAction::BalanceWindows => {
            rt.wm_dwindle_axis_phase = if ks.wm_default_split_axis_vertical {
                0
            } else {
                1
            };
        }
        WmAction::RotateSplit => {
            rt.wm_dwindle_axis_phase = rt.wm_dwindle_axis_phase.wrapping_add(1) % 4;
        }
    }
    apply_window_layout(osr_host, rt);
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut, Resource, World};

#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct StripHudEcsIn(String);

#[cfg(test)]
#[derive(Resource, Default)]
struct StripHudEcsOut(String);

#[cfg(test)]
fn strip_wm_hud_title_system(input: Res<StripHudEcsIn>, mut out: ResMut<StripHudEcsOut>) {
    out.0 = strip_wm_hud_from_title(&input.0);
}

#[cfg(test)]
#[test]
fn strip_hud_roundtrip_via_system() {
    let mut world = World::default();
    world.insert_resource(StripHudEcsIn("Google · [WM ⌃b armed]".to_string()));
    world.insert_resource(StripHudEcsOut::default());
    world.run_system_once(strip_wm_hud_title_system).unwrap();
    assert_eq!(world.resource::<StripHudEcsOut>().0, "Google");
}
