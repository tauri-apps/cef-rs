//! Tmux-style **leader** plus global `[window_manager.tmux.keybindings]` chords (winit path, before Vimium).

use std::time::{Duration, Instant};

use crate::settings::{KeySettings, WmMode};

use bevy_ecs::entity::Entity;
use winit::event::ElementState;
use winit::window::WindowId;

use crate::browser::cef::facet::{EditableFocusSnapshot, editable_focus_is_typing};
use crate::browser::cef::shell::OsrHostState;
use crate::runtime::{AppEvent, RuntimeState, UserEvent, WmAction, send_user_event};
use crate::settings::chord_matches_winit;
use crate::window::wm::{wm_hud_clear, wm_hud_set};

/// When `true`, [`try_handle_wm_keyboard_event`] may run ([`WmMode::Manual`] only).
#[inline]
pub(crate) fn should_handle_tmux_wm_keys(ks: &KeySettings) -> bool {
    ks.wm_mode == WmMode::Manual
}

#[inline]
fn wm_log_debug(fields: &str) {
    bevy_log::debug!(target: "vmux_wm", pid = std::process::id(), "{}", fields);
}

#[inline]
fn wm_log_info(fields: &str) {
    bevy_log::info!(target: "vmux_wm", pid = std::process::id(), "{}", fields);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TmuxLeaderAction {
    Wm(WmAction),
    /// `vertical_bar`: `true` = vertical divider, current pane left / new pane **right**.
    Split {
        vertical_bar: bool,
    },
    NewBrowserWindow,
}

/// **`%`** (Shift+5) → vertical divider, current pane left / new browser **to the right** (tmux
/// “split vertically”). **`"`** (Shift+Quote) → horizontal divider, stacked top/bottom (tmux “split
/// horizontally”).
fn tmux_split_keys_physical(
    phys: &winit::keyboard::PhysicalKey,
    mods: winit::keyboard::ModifiersState,
) -> Option<TmuxLeaderAction> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    if mods.control_key() || mods.super_key() || mods.alt_key() {
        return None;
    }
    match phys {
        PhysicalKey::Code(KeyCode::Quote) if mods.shift_key() => Some(TmuxLeaderAction::Split {
            vertical_bar: false,
        }),
        PhysicalKey::Code(KeyCode::Digit5) if mods.shift_key() => {
            Some(TmuxLeaderAction::Split { vertical_bar: true })
        }
        _ => None,
    }
}

/// Second key after the tmux-style leader.
///
/// Order matters: composed **`"`** / **`%`** from the OS is checked **before** physical Shift+Quote,
/// because `rt.mods_winit` can lag behind `ModifiersChanged` and report **no Shift** on the same
/// keydown as Shift+`"` — which used to route Quote into the plain-letter branch and return `None`.
fn tmux_leader_second_action(
    event: &winit::event::KeyEvent,
    mods: winit::keyboard::ModifiersState,
) -> Option<TmuxLeaderAction> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    if mods.control_key() || mods.super_key() || mods.alt_key() {
        return None;
    }
    let phys = &event.physical_key;

    if let Some(a) = tmux_leader_second_action_from_text(event, mods) {
        return Some(a);
    }

    if let Some(a) = tmux_split_keys_physical(phys, mods) {
        return Some(a);
    }

    match phys {
        PhysicalKey::Code(KeyCode::Quote) | PhysicalKey::Code(KeyCode::Digit5) => None,
        // Require **plain** Space (no Shift/Option/Ctrl/Cmd). Modifier churn (e.g. ⌥⇧Space for IME)
        // can otherwise be read as bare Space and flip tile/stack, leaving OSR out of sync until
        // the next real resize.
        PhysicalKey::Code(KeyCode::Space)
            if !mods.shift_key() && !mods.alt_key() && !mods.control_key() && !mods.super_key() =>
        {
            Some(TmuxLeaderAction::Wm(WmAction::ToggleLayout))
        }
        PhysicalKey::Code(code) if !mods.shift_key() => match code {
            KeyCode::KeyC => Some(TmuxLeaderAction::NewBrowserWindow),
            KeyCode::KeyN | KeyCode::KeyO => Some(TmuxLeaderAction::Wm(WmAction::FocusNext)),
            KeyCode::KeyP => Some(TmuxLeaderAction::Wm(WmAction::FocusPrev)),
            KeyCode::ArrowLeft | KeyCode::ArrowUp => {
                Some(TmuxLeaderAction::Wm(WmAction::FocusPrev))
            }
            KeyCode::ArrowRight | KeyCode::ArrowDown => {
                Some(TmuxLeaderAction::Wm(WmAction::FocusNext))
            }
            _ => None,
        },
        _ => None,
    }
}

fn single_char_from_str(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    if it.next().is_none() { Some(c) } else { None }
}

fn tmux_leader_second_action_from_text(
    event: &winit::event::KeyEvent,
    mods: winit::keyboard::ModifiersState,
) -> Option<TmuxLeaderAction> {
    use winit::keyboard::Key;
    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

    if mods.control_key() || mods.super_key() || mods.alt_key() {
        return None;
    }

    let mut ch_opt = None::<char>;
    for src in [event.text.as_deref(), event.text_with_all_modifiers()] {
        if let Some(s) = src {
            if let Some(c) = single_char_from_str(s) {
                ch_opt = Some(c);
                break;
            }
        }
    }
    if ch_opt.is_none() {
        if let Key::Character(s) = &event.logical_key {
            ch_opt = single_char_from_str(s);
        }
    }

    match ch_opt? {
        '%' => Some(TmuxLeaderAction::Split { vertical_bar: true }),
        '"' => Some(TmuxLeaderAction::Split {
            vertical_bar: false,
        }),
        _ => None,
    }
}

/// Returns `true` if a `[window_manager.tmux.keybindings]` chord was recognized (user event queued).
pub fn try_handle_wm_keyboard_event(
    osr_host: &OsrHostState,
    rt: &mut RuntimeState,
    event: &winit::event::KeyEvent,
    window_id: WindowId,
    ecs_browser: Option<(Entity, i32)>,
    editable_focus: &EditableFocusSnapshot,
) -> bool {
    if event.state != ElementState::Pressed || event.repeat {
        return false;
    }
    let ks = &*osr_host.key_settings;
    let mods = rt.mods_winit;
    let phys = &event.physical_key;

    if let Some(deadline) = rt.wm_tmux_prefix_deadline {
        if Instant::now() > deadline {
            wm_log_info("tmux leader: deadline expired (no second key in time)");
            wm_hud_set(
                rt,
                "⌃b timed out — press prefix again",
                Duration::from_secs(2),
            );
            rt.wm_tmux_prefix_deadline = None;
        }
    }

    let bid = osr_host.browser_id_for_window(window_id, ecs_browser);
    let typing = bid.is_some_and(|b| editable_focus_is_typing(editable_focus, b));

    if rt.wm_tmux_prefix_deadline.is_some() {
        wm_log_debug(&format!(
            "tmux leader: second key candidate window={window_id:?} bid={bid:?} typing={typing} phys={phys:?} shift={} ctrl={} text={:?} logical={:?}",
            mods.shift_key(),
            mods.control_key(),
            event.text.as_deref(),
            event.logical_key
        ));

        if typing {
            wm_log_info(
                "tmux leader: second key ignored for WM (editable typing) — key goes to page; leader STAYS armed",
            );
            wm_hud_set(
                rt,
                "⌃b armed — typing in field; key sent to page. Click page or use insert mode, then \"",
                Duration::from_secs(5),
            );
            return false;
        }

        rt.wm_tmux_prefix_deadline = None;
        wm_hud_clear(rt);

        if let Some(action) = tmux_leader_second_action(event, mods) {
            match action {
                TmuxLeaderAction::NewBrowserWindow => {
                    wm_log_info("tmux leader: action new window");
                    wm_hud_set(rt, "⌃b → new window", Duration::from_secs(1));
                    send_user_event(UserEvent::App(AppEvent::RequestNewBrowserWindow));
                }
                TmuxLeaderAction::Split { vertical_bar } => {
                    wm_log_info(&format!(
                        "tmux leader: RequestSplitPane vertical_bar={vertical_bar} (% = side-by-side right, \" = stacked)"
                    ));
                    wm_hud_set(
                        rt,
                        if vertical_bar {
                            "⌃b → split (pane right)"
                        } else {
                            "⌃b → split (stacked)"
                        },
                        Duration::from_secs(2),
                    );
                    send_user_event(UserEvent::App(AppEvent::RequestSplitPane { vertical_bar }));
                }
                TmuxLeaderAction::Wm(a) => {
                    wm_log_info(&format!("tmux leader: WindowManager {a:?}"));
                    wm_hud_set(rt, &format!("⌃b → {a:?}"), Duration::from_secs(1));
                    send_user_event(UserEvent::App(AppEvent::WindowManager(a)));
                }
            }
            return true;
        }

        wm_log_info("tmux leader: second key had no WM binding (disarmed, key passed through)");
        wm_hud_set(rt, "⌃b — no binding for that key", Duration::from_secs(2));
        return false;
    }

    let hit = |c: &Option<crate::settings::KeyChord>| -> bool {
        c.as_ref()
            .is_some_and(|ch| chord_matches_winit(ch, mods, phys))
    };

    if hit(&ks.wm_new_window) {
        send_user_event(UserEvent::App(AppEvent::RequestNewBrowserWindow));
        return true;
    }
    if hit(&ks.wm_toggle_layout) {
        send_user_event(UserEvent::App(AppEvent::WindowManager(
            WmAction::ToggleLayout,
        )));
        return true;
    }
    if hit(&ks.wm_focus_next) {
        send_user_event(UserEvent::App(AppEvent::WindowManager(WmAction::FocusNext)));
        return true;
    }
    if hit(&ks.wm_focus_prev) {
        send_user_event(UserEvent::App(AppEvent::WindowManager(WmAction::FocusPrev)));
        return true;
    }
    if hit(&ks.wm_split_side_by_side) {
        send_user_event(UserEvent::App(AppEvent::RequestSplitPane {
            vertical_bar: true,
        }));
        return true;
    }
    if hit(&ks.wm_split_stacked) {
        send_user_event(UserEvent::App(AppEvent::RequestSplitPane {
            vertical_bar: false,
        }));
        return true;
    }
    if hit(&ks.wm_balance_windows) {
        send_user_event(UserEvent::App(AppEvent::WindowManager(
            WmAction::BalanceWindows,
        )));
        return true;
    }
    if hit(&ks.wm_rotate_split) {
        send_user_event(UserEvent::App(AppEvent::WindowManager(
            WmAction::RotateSplit,
        )));
        return true;
    }

    if let Some(ref prefix) = ks.wm_tmux_prefix {
        if chord_matches_winit(prefix, mods, phys) {
            if typing {
                wm_log_info("tmux prefix chord matched but ignored (editable typing)");
                return false;
            }
            let ms = ks.wm_tmux_prefix_timeout_ms.max(100).min(60_000);
            rt.wm_tmux_prefix_deadline = Some(Instant::now() + Duration::from_millis(ms));
            wm_log_info(&format!(
                "tmux leader: PREFIX ARMED timeout_ms={ms} — next key: % (side-by-side) | \" (stacked) | c | n/p/o | arrows | Space"
            ));
            wm_hud_set(
                rt,
                "⌃b ARMED — % beside │ \" stack │ c new │ n/p/o focus │ Space layout",
                Duration::from_secs(4),
            );
            return true;
        }
    }

    false
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut, Resource, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;
#[cfg(test)]
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

#[cfg(test)]
#[test]
fn should_handle_tmux_wm_keys_is_true_for_manual_tmux_modes_only() {
    assert!(should_handle_tmux_wm_keys(
        &crate::settings::key_settings_for_test_window_mode("tmux")
    ));
    assert!(should_handle_tmux_wm_keys(
        &crate::settings::key_settings_for_test_window_mode("manual")
    ));
    assert!(!should_handle_tmux_wm_keys(
        &crate::settings::key_settings_for_test_window_mode("dynamic")
    ));
}

#[cfg(test)]
fn shift() -> ModifiersState {
    let mut m = ModifiersState::default();
    m.set(ModifiersState::SHIFT, true);
    m
}

#[cfg(test)]
#[derive(Resource)]
struct TmuxSplitKeysIn {
    key: PhysicalKey,
    mods: ModifiersState,
}

#[cfg(test)]
#[derive(Resource, Default)]
struct TmuxSplitKeysOut(Option<TmuxLeaderAction>);

#[cfg(test)]
fn tmux_split_keys_populate_system(inp: Res<TmuxSplitKeysIn>, mut out: ResMut<TmuxSplitKeysOut>) {
    out.0 = tmux_split_keys_physical(&inp.key, inp.mods);
}

#[cfg(test)]
#[test]
fn tmux_leader_shift_quote_stacks_top_bottom_via_system() {
    let mut world = World::default();
    world.insert_resource(TmuxSplitKeysIn {
        key: PhysicalKey::Code(KeyCode::Quote),
        mods: shift(),
    });
    world.insert_resource(TmuxSplitKeysOut::default());
    world
        .run_system_once(tmux_split_keys_populate_system)
        .unwrap();
    assert_eq!(
        world.resource::<TmuxSplitKeysOut>().0,
        Some(TmuxLeaderAction::Split {
            vertical_bar: false
        })
    );
}

#[cfg(test)]
#[test]
fn tmux_leader_shift5_percent_opens_pane_right_vertical_split_via_system() {
    let mut world = World::default();
    world.insert_resource(TmuxSplitKeysIn {
        key: PhysicalKey::Code(KeyCode::Digit5),
        mods: shift(),
    });
    world.insert_resource(TmuxSplitKeysOut::default());
    world
        .run_system_once(tmux_split_keys_populate_system)
        .unwrap();
    assert_eq!(
        world.resource::<TmuxSplitKeysOut>().0,
        Some(TmuxLeaderAction::Split { vertical_bar: true })
    );
}
