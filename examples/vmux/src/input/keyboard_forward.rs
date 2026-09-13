//! Forward winit `KeyboardInput` to CEF after vimium / WM / OS shortcuts in [`crate::window::dispatch`].
//!
//! Part of the **input** domain ([`super::InputPlugin`]): enqueue/drain helpers for shell→CEF
//! keys, alongside [`super::shell::OsrKeyboardShellSnapshot`].
//!
//! Events are enqueued from [`crate::window::dispatch::handle_window_event_inner`] (and from
//! [`crate::tmux::system::process_tmux_wm_shell_keyboard_events_system`] for manual WM keys) and
//! drained after **each** dispatch / tmux event so modifier and link-hint snapshots stay aligned.

use std::collections::VecDeque;

use cef::sys::cef_event_flags_t;
use cef::{
    ImplBrowser as _, ImplBrowserHost as _, ImplFrame as _, KeyEvent as CefKeyEvent, KeyEventType,
};
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::WindowId;

use crate::browser::cef::focus;
use crate::browser::cef::input::keyboard;
use crate::browser::cef::queue::{BrowserUiOp, enqueue_browser_ui_op};
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::BrowserEventBatch;
use crate::input::shell::{self as shell_input, OsrKeyboardShellSnapshot};
use crate::runtime::RuntimeState;

/// Modifier and link-hint state captured when the winit event is handled.
pub(crate) struct PendingOsrKeyboardForward {
    pub window_id: WindowId,
    pub browser_id: i32,
    pub event: KeyEvent,
    pub mods_winit: ModifiersState,
    /// CEF modifier flags at dispatch time (`rt.mods`, aligned with `mods_winit` after reorder).
    pub cef_mods: cef_event_flags_t,
    pub link_hints_active: bool,
    pub link_hints_typed_prefix_len: usize,
}

impl PendingOsrKeyboardForward {
    pub(crate) fn capture(
        window_id: WindowId,
        browser_id: i32,
        event: KeyEvent,
        rt: &RuntimeState,
        vim: &(impl OsrKeyboardShellSnapshot + ?Sized),
    ) -> Self {
        Self {
            window_id,
            browser_id,
            event,
            mods_winit: rt.mods_winit,
            cef_mods: rt.mods,
            link_hints_active: vim.link_hints_active(),
            link_hints_typed_prefix_len: vim.link_hints_typed_prefix_len(),
        }
    }
}

pub(crate) fn enqueue_osr_keyboard_forward(
    pending: &mut VecDeque<PendingOsrKeyboardForward>,
    window_id: WindowId,
    browser_id: i32,
    event: KeyEvent,
    rt: &RuntimeState,
    vim: &(impl OsrKeyboardShellSnapshot + ?Sized),
) {
    pending.push_back(PendingOsrKeyboardForward::capture(
        window_id, browser_id, event, rt, vim,
    ));
}

pub(crate) fn drain_pending_osr_keyboard_forwards(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    pending: &mut VecDeque<PendingOsrKeyboardForward>,
    out: &mut BrowserEventBatch,
) {
    while let Some(entry) = pending.pop_front() {
        apply_one_osr_keyboard_forward(osr_host, rt, entry, out);
    }
}

fn apply_one_osr_keyboard_forward(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    entry: PendingOsrKeyboardForward,
    out: &mut BrowserEventBatch,
) {
    let PendingOsrKeyboardForward {
        window_id,
        browser_id: bid,
        event,
        mods_winit,
        cef_mods,
        link_hints_active,
        link_hints_typed_prefix_len: prior_typed_len_snapshot,
    } = entry;

    let ctrl = mods_winit.control_key();
    let cmd = mods_winit.super_key();
    let alt = mods_winit.alt_key();

    let Ok(windows) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };
    let Some(store_entry) = windows.get(&window_id) else {
        return;
    };
    if store_entry.browser.identifier() != bid {
        return;
    }
    let browser = store_entry.browser.clone();
    drop(windows);

    if let Some(h) = browser.host() {
        h.set_focus(1);
    }

    let hint_ch = keyboard::hint_label_char_from_key_event(&event);

    if link_hints_active {
        if let Some(ch) = hint_ch {
            if !ctrl && !cmd && !alt {
                match event.state {
                    ElementState::Pressed => {
                        if event.repeat {
                            return;
                        }
                        out.link_hints_feed_key_deferred.push(
                            crate::browser::event::LinkHintsFeedKeyDeferredBrowserEvent {
                                browser_id: bid,
                                ch,
                                prior_typed_len: prior_typed_len_snapshot,
                            },
                        );
                        return;
                    }
                    ElementState::Released => {
                        return;
                    }
                }
            }
        }
    }

    let Ok(windows) = osr_host.cef_attach.windows_store.lock() else {
        return;
    };
    let Some(store_entry) = windows.get(&window_id) else {
        return;
    };
    let Some(host) = store_entry.browser.host() else {
        return;
    };

    let hints_active = link_hints_active;

    if !hints_active && event.state == ElementState::Pressed {
        if let PhysicalKey::Code(code) = event.physical_key {
            if cmd && code == KeyCode::KeyA {
                if let Some(frame) = store_entry
                    .browser
                    .focused_frame()
                    .or_else(|| store_entry.browser.main_frame())
                {
                    frame.select_all();
                }
                return;
            }

            if ctrl {
                let mapped: Option<(i32, i32, u16)> = match code {
                    KeyCode::KeyA => Some((0x24, 0x73, 0)),
                    KeyCode::KeyE => Some((0x23, 0x77, 0)),
                    KeyCode::KeyB => Some((0x25, 0x7B, 0)),
                    KeyCode::KeyF => Some((0x27, 0x7C, 0)),
                    KeyCode::KeyP => Some((0x26, 0x7E, 0)),
                    KeyCode::KeyN => Some((0x28, 0x7D, 0)),
                    _ => None,
                };
                if let Some((vk, native, ch)) = mapped {
                    keyboard::send_key_press_and_release(
                        &host,
                        cef_event_flags_t::EVENTFLAG_NONE,
                        vk,
                        native,
                        ch,
                    );
                    return;
                }
            }
        }
    }

    if let Some((vk, native, key_char_u16)) = keyboard::key_codes_from_physical(&event.physical_key)
    {
        match event.state {
            ElementState::Pressed => {
                shell_input::trace_cef_keydown_forward_from_winit(bid, &event, vk);
                let types: &[KeyEventType] = match event.physical_key {
                    PhysicalKey::Code(
                        KeyCode::Backspace
                        | KeyCode::Delete
                        | KeyCode::Enter
                        | KeyCode::NumpadEnter,
                    ) => &[KeyEventType::KEYDOWN],
                    _ => &[KeyEventType::RAWKEYDOWN, KeyEventType::KEYDOWN],
                };
                for type_ in types {
                    let kev = CefKeyEvent {
                        type_: *type_,
                        modifiers: cef_mods.0,
                        windows_key_code: vk,
                        native_key_code: native,
                        is_system_key: 0,
                        character: key_char_u16,
                        unmodified_character: key_char_u16,
                        focus_on_editable_field: 1,
                        ..Default::default()
                    };
                    host.send_key_event(Some(&kev));
                }
                if matches!(
                    event.physical_key,
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter)
                ) {
                    rt.bump_cef_post_create_pumps(24);
                }
            }
            ElementState::Released => {
                let kev = CefKeyEvent {
                    type_: KeyEventType::KEYUP,
                    modifiers: cef_mods.0,
                    windows_key_code: vk,
                    native_key_code: native,
                    is_system_key: 0,
                    character: key_char_u16,
                    unmodified_character: key_char_u16,
                    focus_on_editable_field: 1,
                    ..Default::default()
                };
                host.send_key_event(Some(&kev));
            }
        }
    }

    if event.state == ElementState::Pressed {
        let has_shortcut_mod = mods_winit.super_key() || mods_winit.control_key();
        if !hints_active && !has_shortcut_mod {
            if let Some(text) = &event.text {
                shell_input::trace_cef_send_char_forward_from_winit(bid, text.as_str());
                let mut any = false;
                for ch in text.chars() {
                    if keyboard::keyboard_text_char_duplicates_keydown(ch) {
                        continue;
                    }
                    keyboard::send_char(&host, cef_mods, ch);
                    any = true;
                }
                if any {
                    enqueue_browser_ui_op(
                        &osr_host.browser_ui_ops,
                        BrowserUiOp::SetEditableFocusHint {
                            browser_id: bid,
                            editable: true,
                        },
                    );
                }
            }
        }
    }

    if event.state == ElementState::Pressed && !hints_active {
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => {
                enqueue_browser_ui_op(
                    &osr_host.browser_ui_ops,
                    BrowserUiOp::InvalidateEditableFocusHint { browser_id: bid },
                );
                focus::enqueue_probe_request(&osr_host.editable_focus_queues.pending_probes, bid);
            }
            _ => {}
        }
    }
}
