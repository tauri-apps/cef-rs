//! Winit [`WindowEvent`] dispatch into CEF OSR — implementation lives here.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;

use bevy_ecs::entity::Entity;
use cef::*;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::WindowId;

use crate::browser::cef as vmux_cef;
use crate::browser::cef::focus;
use crate::browser::cef::input::{keyboard, mouse};
use crate::browser::cef::queue::{BrowserUiOp, enqueue_browser_ui_op};
use crate::runtime::LinkHintsNavPending;
use crate::runtime::RuntimeState;

use crate::browser::cef::facet::EditableFocusSnapshot;
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::BrowserEventBatch;
use crate::input::shell::{self as shell_input, ShellKeyboardHandler};
use crate::input::keyboard_forward::{PendingOsrKeyboardForward, enqueue_osr_keyboard_forward};

fn update_mods_from_winit(_osr_host: &mut OsrHostState, rt: &mut RuntimeState, m: ModifiersState) {
    rt.mods_winit = m;
    rt.mods = keyboard::update_mods_from_winit(m);
}

pub(crate) fn window_dispatch_prelude(
    osr_host: &mut OsrHostState,
    vim: &mut dyn ShellKeyboardHandler,
    link_hints_nav_pending: &mut LinkHintsNavPending,
    out: &mut BrowserEventBatch,
) {
    osr_host.apply_pending_titles();
    let link_nav_ids = std::mem::take(&mut link_hints_nav_pending.0);
    vim.apply_link_hints_navigation_resets(osr_host, out, link_nav_ids);
}

pub(crate) fn try_enqueue_shell_keyboard_forward_to_cef(
    osr_host: &mut OsrHostState,
    window_id: WindowId,
    key_event: winit::event::KeyEvent,
    rt: &RuntimeState,
    vim: &dyn ShellKeyboardHandler,
    keyboard_forward_pending: &mut VecDeque<PendingOsrKeyboardForward>,
) {
    use cef::ImplBrowser;
    let Ok(windows) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };
    let Some(entry) = windows.get(&window_id) else {
        return;
    };
    let bid = entry.browser.identifier();
    drop(windows);
    enqueue_osr_keyboard_forward(
        keyboard_forward_pending,
        window_id,
        bid,
        key_event,
        rt,
        vim,
    );
}

pub(crate) fn handle_window_event_inner(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut dyn ShellKeyboardHandler,
    window_id: WindowId,
    event: WindowEvent,
    ecs_browser: Option<(Entity, i32)>,
    editable_focus: &EditableFocusSnapshot,
    out: &mut BrowserEventBatch,
    keyboard_forward_pending: &mut VecDeque<PendingOsrKeyboardForward>,
) {
    // Update modifier flags without holding the windows_store lock.
    if let WindowEvent::ModifiersChanged(m) = &event {
        update_mods_from_winit(osr_host, rt, m.state());
    }

    // Vimium (`settings.toml` `[vimium]`, …). Manual WM chords use [`crate::tmux::event::TmuxWmShellKeyboardEvent`].
    if let WindowEvent::KeyboardInput { event, .. } = &event {
        let vimium_handled = vim.try_handle_vimium_keys(
            osr_host,
            rt,
            window_id,
            event,
            ecs_browser,
            editable_focus,
            out,
        );
        if vimium_handled {
            return;
        }
    }

    // History: **Cmd+[** / **Cmd+]** (macOS), same as Chromium window shortcuts. Unlike Shift+H/L,
    // we do **not** consult the editable-focus hint so back/forward still run from search fields
    // and other inputs.
    if let WindowEvent::KeyboardInput { event, .. } = &event {
        if event.state == ElementState::Pressed {
            let primary = rt.mods_winit.super_key();
            if primary {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let go_forward = match code {
                        KeyCode::BracketLeft => Some(false),
                        KeyCode::BracketRight => Some(true),
                        _ => None,
                    };
                    if let Some(go_forward) = go_forward {
                        if let Some(bid) = osr_host.browser_id_for_window(window_id, ecs_browser) {
                            osr_host.request_set_active_browser(bid);
                            out.navigate
                                .push(crate::browser::event::NavigateBrowserEvent {
                                    browser_id: bid,
                                    go_forward,
                                });
                            return;
                        }
                    }
                }
            }
        }
    }

    // **Alt+Left** / **Alt+Right** — typical browser back/forward; no editable probe.
    if let WindowEvent::KeyboardInput { event, .. } = &event {
        if event.state == ElementState::Pressed {
            let alt = rt.mods_winit.alt_key();
            let cmd = rt.mods_winit.super_key();
            let ctrl = rt.mods_winit.control_key();
            if alt && !cmd && !ctrl {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let go_forward = match code {
                        KeyCode::ArrowLeft => Some(false),
                        KeyCode::ArrowRight => Some(true),
                        _ => None,
                    };
                    if let Some(go_forward) = go_forward {
                        if let Some(bid) = osr_host.browser_id_for_window(window_id, ecs_browser) {
                            osr_host.request_set_active_browser(bid);
                            out.navigate
                                .push(crate::browser::event::NavigateBrowserEvent {
                                    browser_id: bid,
                                    go_forward,
                                });
                            return;
                        }
                    }
                }
            }
        }
    }

    // **Cmd+R** — same as Chrome; no editable-focus probe so reload works from inputs. Vimium uses
    // **`shift+r`** (`R`) for a letter-only reload.
    if let WindowEvent::KeyboardInput { event, .. } = &event {
        if event.state == ElementState::Pressed {
            let primary = rt.mods_winit.super_key();
            if primary && !rt.mods_winit.alt_key() {
                if let PhysicalKey::Code(KeyCode::KeyR) = event.physical_key {
                    if let Some(bid) = osr_host.browser_id_for_window(window_id, ecs_browser) {
                        osr_host.request_set_active_browser(bid);
                        out.reload
                            .push(crate::browser::event::ReloadBrowserEvent { browser_id: bid });
                        return;
                    }
                }
            }
        }
    }

    // Global app shortcuts that shouldn't depend on the current browser/window entry.
    if let WindowEvent::KeyboardInput { event: key, .. } = &event {
        let cmd = rt.mods_winit.super_key();
        if cmd && key.state == ElementState::Pressed {
            match key.physical_key {
                PhysicalKey::Code(KeyCode::KeyQ) => {
                    // Quit: force-close all browsers. The normal shutdown path is driven by
                    // `on_before_close` setting the shutdown flag once the last browser closes.
                    crate::log::record_runtime_event(
                        "window_dispatch Cmd+Q quit_close_all_browsers queued",
                    );
                    crate::browser::cef::quit::try_begin_quit_visual_feedback();
                    rt.quit_requested = true;
                    out.quit_close_all_browsers
                        .push(crate::browser::event::QuitCloseAllBrowsersBrowserEvent);
                    return;
                }
                _ => {}
            }
        }
    }

    let Ok(mut windows) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };

    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            let Some(entry) = windows.get(&window_id) else {
                return;
            };
            let bid = entry.browser.identifier();
            drop(windows);
            enqueue_osr_keyboard_forward(
                keyboard_forward_pending,
                window_id,
                bid,
                event.clone(),
                rt,
                &*vim,
            );
        }
        WindowEvent::Ime(ime) => {
            let Some(entry) = windows.get(&window_id) else {
                return;
            };
            let browser = entry.browser.clone();
            drop(windows);

            let Some(host) = browser.host() else {
                return;
            };
            host.set_focus(1);
            let bid = browser.identifier();
            shell_input::trace_winit_ime_event(window_id, bid, &ime);
            if let winit::event::Ime::Commit(text) = &ime {
                let vimium_handled = vim.try_handle_vimium_ime_commit(
                    osr_host,
                    rt,
                    window_id,
                    bid,
                    text.as_str(),
                    editable_focus,
                    out,
                );
                if vimium_handled {
                    return;
                }
            }
            // `KeyboardInput` for hint letters is swallowed, but macOS still emits `Ime::Commit`
            // for the same physical key; forwarding it would inject into the page (e.g. second
            // letter of hint "by") and sites like Ledger show "press / for search" when focus
            // churns.
            match &ime {
                winit::event::Ime::Commit(_) => {
                    if vim.link_hints_active() && vim.link_hints_browser_id() == Some(bid) {
                        return;
                    }
                    enqueue_browser_ui_op(
                        &osr_host.browser_ui_ops,
                        BrowserUiOp::SetEditableFocusHint {
                            browser_id: bid,
                            editable: true,
                        },
                    );
                }
                winit::event::Ime::Disabled => {
                    enqueue_browser_ui_op(
                        &osr_host.browser_ui_ops,
                        BrowserUiOp::InvalidateEditableFocusHint { browser_id: bid },
                    );
                    focus::enqueue_probe_request(
                        &osr_host.editable_focus_queues.pending_probes,
                        bid,
                    );
                }
                _ => {
                    enqueue_browser_ui_op(
                        &osr_host.browser_ui_ops,
                        BrowserUiOp::SetEditableFocusHint {
                            browser_id: bid,
                            editable: true,
                        },
                    );
                }
            }
            if let winit::event::Ime::Commit(text) = ime {
                shell_input::trace_ime_commit_forward_to_cef(bid, text.as_str());
                for ch in text.chars() {
                    keyboard::send_char(&host, rt.mods, ch);
                }
            }
        }
        WindowEvent::CursorMoved { position, .. } => {
            if let Some(entry) = windows.get(&window_id) {
                if let Some(host) = entry.browser.host() {
                    // CEF OSR expects DIP (logical) coordinates.
                    rt.last_cursor_pos =
                        mouse::cursor_moved_dip(position, entry.surface.window.scale_factor());
                    let ev = MouseEvent {
                        x: rt.last_cursor_pos.0,
                        y: rt.last_cursor_pos.1,
                        modifiers: rt.mods.0,
                    };
                    host.send_mouse_move_event(Some(&ev), 0);
                }
            }
        }
        WindowEvent::CursorLeft { .. } => {
            rt.primary_mouse_down = false;
            if let Some(entry) = windows.get(&window_id) {
                if let Some(host) = entry.browser.host() {
                    let ev = MouseEvent {
                        x: rt.last_cursor_pos.0,
                        y: rt.last_cursor_pos.1,
                        modifiers: rt.mods.0,
                    };
                    host.send_mouse_move_event(Some(&ev), 1);
                }
            }
        }
        WindowEvent::Focused(focused) => {
            if !focused {
                rt.primary_mouse_down = false;
            }
            #[cfg(target_os = "macos")]
            if !focused {
                // Winit/AppKit often emits a transient blur while the osr_host window is still the
                // right target (IME, key-window churn). Telling CEF `set_focus(0)` clears the
                // focused `<input>`, caret, and selection — avoid that for windowless OSR.
                // Re-assert browser focus only (do not `focus_window` here — that would steal
                // activation when the user intentionally switched to another app).
                if let Some(entry) = windows.get(&window_id) {
                    if let Some(host) = entry.browser.host() {
                        host.set_focus(1);
                    }
                }
                return;
            }
            if let Some(entry) = windows.get(&window_id) {
                if let Some(host) = entry.browser.host() {
                    host.set_focus(focused.into());
                    if focused {
                        rt.wm_focused_shell_window = Some(window_id);
                        osr_host.request_set_active_browser(entry.browser.identifier());
                    }
                }
            }
        }
        WindowEvent::ModifiersChanged(_m) => {}
        WindowEvent::MouseInput { state, button, .. } => {
            let Some(entry) = windows.get(&window_id) else {
                return;
            };
            match (state, button) {
                (ElementState::Pressed, MouseButton::Back) => {
                    let bid = entry.browser.identifier();
                    drop(windows);
                    osr_host.request_set_active_browser(bid);
                    out.navigate
                        .push(crate::browser::event::NavigateBrowserEvent {
                            browser_id: bid,
                            go_forward: false,
                        });
                    return;
                }
                (ElementState::Pressed, MouseButton::Forward) => {
                    let bid = entry.browser.identifier();
                    drop(windows);
                    osr_host.request_set_active_browser(bid);
                    out.navigate
                        .push(crate::browser::event::NavigateBrowserEvent {
                            browser_id: bid,
                            go_forward: true,
                        });
                    return;
                }
                _ => {}
            }
            let Some(entry) = windows.get(&window_id) else {
                return;
            };
            #[cfg(target_os = "macos")]
            let window = entry.surface.window.clone();
            let Some(host) = entry.browser.host() else {
                return;
            };
            match state {
                ElementState::Pressed => {
                    // Become key *before* CEF sees the click so nested focus logic sees our window.
                    #[cfg(target_os = "macos")]
                    if !window.has_focus() {
                        window.focus_window();
                    }
                    host.set_focus(1);
                    osr_host.request_set_active_browser(entry.browser.identifier());
                }
                ElementState::Released => {}
            }
            let cef_button = match button {
                MouseButton::Left => MouseButtonType::LEFT,
                MouseButton::Right => MouseButtonType::RIGHT,
                MouseButton::Middle => MouseButtonType::MIDDLE,
                _ => return,
            };
            match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => {
                    rt.primary_mouse_down = true;
                }
                (MouseButton::Left, ElementState::Released) => {
                    rt.primary_mouse_down = false;
                }
                _ => {}
            }
            let mouse_up = match state {
                ElementState::Released => 1,
                _ => 0,
            };
            let ev = MouseEvent {
                x: rt.last_cursor_pos.0,
                y: rt.last_cursor_pos.1,
                modifiers: rt.mods.0,
            };
            host.send_mouse_click_event(Some(&ev), cef_button, mouse_up, 1);
            // Re-hit-test hover/cursor after click so `on_cursor_change` runs (I-beam on inputs).
            if cef_button == MouseButtonType::LEFT && mouse_up == 1 {
                host.send_mouse_move_event(Some(&ev), 0);
                let bid = entry.browser.identifier();
                drop(windows);
                enqueue_browser_ui_op(
                    &osr_host.browser_ui_ops,
                    BrowserUiOp::InvalidateEditableFocusHint { browser_id: bid },
                );
                // Async probe alone can finish after the first keystroke, so `d`/`g`/`r` vimium
                // bindings still run with a stale "not editable" hint — sync refresh before typing.
                focus::enqueue_probe_request(&osr_host.editable_focus_queues.pending_probes, bid);
                return;
            }
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let Some(entry) = windows.get(&window_id) else {
                return;
            };
            let browser = entry.browser.clone();
            drop(windows);

            let mods = rt.mods;
            let (dx, dy, mods) = mouse::wheel_to_cef(delta, mods);

            let Some(host) = browser.host() else {
                return;
            };
            let Some((dx_i, dy_i)) = mouse::take_wheel_deltas_i32(&mut rt.wheel_residual, dx, dy)
            else {
                return;
            };
            let ev = MouseEvent {
                x: rt.last_cursor_pos.0,
                y: rt.last_cursor_pos.1,
                modifiers: mods.0,
            };
            host.send_mouse_wheel_event(Some(&ev), dx_i, dy_i);
        }
        WindowEvent::CloseRequested => {
            if windows.contains_key(&window_id) {
                drop(windows);
                out.arm_windowless_close
                    .push(crate::browser::event::ArmWindowlessCloseBrowserEvent);
                let Ok(windows) = osr_host.cef_attach.windows_store.lock() else {
                    return;
                };
                if let Some(entry) = windows.get(&window_id) {
                    if let Some(host) = entry.browser.host() {
                        host.try_close_browser();
                    }
                }
            }
        }
        WindowEvent::RedrawRequested => {
            #[cfg(feature = "accelerated_osr")]
            if let Some(entry) = windows.get_mut(&window_id) {
                if let Some(host) = entry.browser.host() {
                    host.send_external_begin_frame();
                }
            }
            // Shell may exist before `on_after_created` — still queue paint so the window is not blank.
            rt.vmux_osr_redraw_queue.push_back(window_id);
        }
        WindowEvent::Resized(physical) => {
            if let Some(entry) = windows.get_mut(&window_id) {
                entry.surface.resize(&*crate::runtime::ffi_gpu(), physical);
                vmux_cef::set_device_scale_factor(entry.surface.window.scale_factor() as f32);
                let logical = physical.to_logical(entry.surface.window.scale_factor());
                if let Ok(mut s) = entry.size.lock() {
                    *s = logical;
                }
                if let Some(host) = entry.browser.host() {
                    host.was_resized();
                    host.notify_screen_info_changed();
                    host.invalidate(PaintElementType::default());
                }
            } else if let Some(p) = rt
                .pending_browser_hosts
                .iter_mut()
                .find(|p| p.surface.window.id() == window_id)
            {
                p.surface.resize(&*crate::runtime::ffi_gpu(), physical);
                vmux_cef::set_device_scale_factor(p.surface.window.scale_factor() as f32);
                p.logical = physical.to_logical(p.surface.window.scale_factor());
                p.surface.window.request_redraw();
            }
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor: _,
            inner_size_writer: _,
        } => {
            // macOS: this fires on backing scale changes (and sometimes during resizes).
            // Treat it like a resize to keep CEF's view rect and our surface in sync.
            let new_physical = if let Some(entry) = windows.get(&window_id) {
                entry.surface.window.inner_size()
            } else if let Some(p) = rt
                .pending_browser_hosts
                .iter_mut()
                .find(|p| p.surface.window.id() == window_id)
            {
                p.surface.window.inner_size()
            } else {
                return;
            };
            if let Some(entry) = windows.get_mut(&window_id) {
                entry
                    .surface
                    .resize(&*crate::runtime::ffi_gpu(), new_physical);
                vmux_cef::set_device_scale_factor(entry.surface.window.scale_factor() as f32);
                let logical = new_physical.to_logical(entry.surface.window.scale_factor());
                if let Ok(mut s) = entry.size.lock() {
                    *s = logical;
                }
                if let Some(host) = entry.browser.host() {
                    host.notify_screen_info_changed();
                    host.was_resized();
                    host.invalidate(PaintElementType::default());
                }
                entry.surface.window.request_redraw();
            } else if let Some(p) = rt
                .pending_browser_hosts
                .iter_mut()
                .find(|p| p.surface.window.id() == window_id)
            {
                p.surface.resize(&*crate::runtime::ffi_gpu(), new_physical);
                vmux_cef::set_device_scale_factor(p.surface.window.scale_factor() as f32);
                p.logical = new_physical.to_logical(p.surface.window.scale_factor());
                p.surface.window.request_redraw();
            }
        }
        WindowEvent::Destroyed => {
            // The NSWindow can go away while `CefBrowserHandles` still holds a `Browser`
            // clone (we never called `try_close_browser`, or the system closed the window). Then
            // CEF keeps helpers alive and `on_before_close` may not run with an empty list, so
            // `shutdown` stays false and the main loop spins forever. Force-close the browser to
            // drive `on_before_close` and helper teardown.
            let removed_attached = if let Some(entry) = windows.remove(&window_id) {
                let bid = entry.browser.identifier();
                bevy_log::info!(
                    target: "vmux",
                    pid = std::process::id(),
                    "WindowEvent::Destroyed: winit window lost, force CEF close browser_id={bid}"
                );
                if let Some(host) = entry.browser.host() {
                    host.close_browser(1);
                }
                crate::browser::cef::osr::unregister_tab(
                    crate::runtime::ffi_osr_index().as_ref(),
                    bid,
                );
                true
            } else {
                false
            };
            if !removed_attached {
                drop(windows);
                let before = rt.pending_browser_hosts.len();
                rt.pending_browser_hosts
                    .retain(|p| p.surface.window.id() != window_id);
                let removed = before.saturating_sub(rt.pending_browser_hosts.len());
                for _ in 0..removed {
                    osr_host
                        .cef_attach
                        .unpaired_cef_shells
                        .fetch_sub(1, Ordering::Release);
                }
                if removed > 0 {
                    bevy_log::info!(
                        target: "vmux",
                        pid = std::process::id(),
                        "WindowEvent::Destroyed: removed pending osr_host (browser not attached yet)"
                    );
                } else {
                    bevy_log::info!(
                        target: "vmux",
                        pid = std::process::id(),
                        "WindowEvent::Destroyed: unknown WindowId (no map entry, no pending osr_host) {window_id:?}"
                    );
                }
                return;
            }
        }
        _ => {}
    }
}
