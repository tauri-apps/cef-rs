//! Vimium key handling, find-mode queue, and link-hint UX (winit → CEF osr_host).
//! Lives under the top-level [`crate::vimium`] module; consumed by [`crate::window::dispatch`]
//! and [`crate::vimium::VimiumPlugin`] systems.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy_ecs::entity::Entity;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::WindowId;

use super::editable_gating::{self as editable_gating};
use super::modes;
use super::scroll;
use super::state::{VimiumChromeCleanup, VimiumState};

use crate::browser::cef::facet::EditableFocusSnapshot;
use crate::browser::cef::focus;
use crate::browser::cef::frames::blur_active_element_all_frames;
use crate::browser::cef::input::keyboard;
use crate::browser::cef::lookup::cef_browser_by_id;
use crate::browser::cef::queue::{BrowserUiOp, enqueue_browser_ui_op};
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::{
    BrowserEventBatch, LinkHintsFeedKeyDeferredBrowserEvent, LinkHintsHideBrowserEvent,
    LinkHintsShowBrowserEvent, NavigateBrowserEvent, ReloadBrowserEvent,
};
use crate::input::shell::{self as shell_input, OsrKeyboardShellSnapshot, ShellKeyboardHandler};
use crate::runtime::RuntimeState;
use crate::settings::{
    chord_matches_winit, chord_matches_winit_ime_char, chord_matches_winit_or_text,
};

/// OSR [`windows_store`] is keyed by [`WindowId`]; on the first frames after attach, ECS may already
/// know `browser_id` while `get(window_id)` still misses. Vimium replay then must not drop deferred
/// keys — fall back to [`crate::browser::cef::lookup::cef_browser_by_id`].
fn browser_for_vimium_target(
    osr_host: &OsrHostState,
    window_id: WindowId,
    browser_id: i32,
) -> Option<cef::Browser> {
    osr_host
        .browser_for_window(window_id)
        .or_else(|| cef_browser_by_id(browser_id))
}

/// Consent / language controls can keep DOM focus on a `<button>`; blur once so Esc / vimium work.
/// Does not run in insert / find / visual / link-hints modes. Caller still forwards keys to CEF if needed.
fn blur_trapped_dom_focus_browse_mode(
    osr_host: &mut OsrHostState,
    window_id: WindowId,
    bid: i32,
    vim: &VimiumState,
) {
    if vim.link_hints_active() || vim.is_insert(bid) || vim.is_find(bid) || vim.is_visual(bid) {
        return;
    }
    let Some(b) = browser_for_vimium_target(osr_host, window_id, bid) else {
        return;
    };
    blur_active_element_all_frames(&b);
    enqueue_browser_ui_op(
        &osr_host.browser_ui_ops,
        BrowserUiOp::InvalidateEditableFocusHint { browser_id: bid },
    );
    focus::enqueue_probe_request(&osr_host.editable_focus_queues.pending_probes, bid);
    osr_host.nudge_osr_view_after_input(window_id);
}

pub(crate) fn queue_find_mode_key(
    _osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    window_id: WindowId,
    event: winit::event::KeyEvent,
) {
    rt.pending_find_mode_keys.push_back((window_id, event));
}

pub(crate) fn drain_find_mode_keys(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
) {
    while let Some((window_id, event)) = rt.pending_find_mode_keys.pop_front() {
        let _ = handle_find_mode_pressed_inner(osr_host, rt, vim, window_id, &event);
    }
}

pub(crate) fn defer_vimium_key_after_editable_probe(
    osr_host: &mut OsrHostState,
    window_id: WindowId,
    browser_id: i32,
    event: &winit::event::KeyEvent,
) -> bool {
    focus::enqueue_shortcut_key_replay_request(
        &osr_host.editable_focus_queues.pending_shortcut_key_replays,
        browser_id,
        window_id,
        event.clone(),
    );
    true
}

pub(crate) fn apply_link_hint_feed_command(
    osr_host: &mut OsrHostState,
    vim: &mut VimiumState,
    window_id: WindowId,
    browser_id: i32,
    ch: char,
    outcome_still_active: bool,
    _hint_label_width: u8,
    out: &mut BrowserEventBatch,
) {
    if vim.link_hints_browser_id() != Some(browser_id) {
        return;
    }
    if outcome_still_active {
        vim.link_hints_push_typed_char(ch);
    } else {
        out.link_hints_hide
            .push(LinkHintsHideBrowserEvent { browser_id });
        vim.clear_link_hints();
        enqueue_browser_ui_op(
            &osr_host.browser_ui_ops,
            BrowserUiOp::InvalidateEditableFocusHint { browser_id },
        );
        focus::enqueue_probe_request(&osr_host.editable_focus_queues.pending_probes, browser_id);
    }
    osr_host.nudge_osr_view_after_input(window_id);
}
fn discard_vimium_link_hints_for_browser_if_any(
    osr_host: &mut OsrHostState,
    vim: &mut VimiumState,
    bid: i32,
    out: &mut BrowserEventBatch,
) {
    vim.clear_link_hints_if_browser(bid);
    let Some(wid) = crate::browser::cef::osr::window_id_for_browser(
        crate::runtime::ffi_osr_index().as_ref(),
        bid,
    ) else {
        return;
    };
    // Always hide on navigation invalidation: Rust may already be `Browse` while the DOM
    // overlay remains (e.g. missed sync), which breaks a follow-up `f`.
    out.link_hints_hide
        .push(LinkHintsHideBrowserEvent { browser_id: bid });
    osr_host.nudge_osr_view_after_input(wid);
}

fn apply_link_hints_navigation_resets_impl(
    osr_host: &mut OsrHostState,
    vim: &mut VimiumState,
    out: &mut BrowserEventBatch,
    browser_ids: Vec<i32>,
) {
    for bid in browser_ids {
        discard_vimium_link_hints_for_browser_if_any(osr_host, vim, bid, out);
        discard_vimium_ux_for_browser_if_any(osr_host, vim, bid);
    }
}

fn cleanup_vimium_ux_ui_at_window(osr_host: &OsrHostState, vim: &VimiumState, window_id: WindowId) {
    let Some(b) = osr_host.browser_for_window(window_id) else {
        return;
    };
    match vim.chrome_cleanup() {
        Some(VimiumChromeCleanup::Find) => {
            modes::find_ui_hide(&b);
            modes::cef_stop_finding(&b, true);
        }
        Some(VimiumChromeCleanup::Visual) => modes::visual_hint_hide(&b),
        None => {}
    }
}

fn clear_vimium_ux_if_other_browser(osr_host: &mut OsrHostState, vim: &mut VimiumState, bid: i32) {
    let Some(old_bid) = vim.ux_browser_id() else {
        return;
    };
    if old_bid == bid {
        return;
    }
    discard_vimium_ux_for_browser_if_any(osr_host, vim, old_bid);
}

/// End insert / find / visual for this browser without clearing `find_committed` (same-tab mode switch).
pub(crate) fn teardown_vimium_ux_at_window_keep_committed(
    osr_host: &mut OsrHostState,
    vim: &mut VimiumState,
    window_id: WindowId,
    bid: i32,
) {
    if vim.ux_browser_id() != Some(bid) {
        return;
    }
    cleanup_vimium_ux_ui_at_window(osr_host, vim, window_id);
    vim.exit_ux_to_browse();
}

fn discard_vimium_ux_for_browser_if_any(
    osr_host: &mut OsrHostState,
    vim: &mut VimiumState,
    bid: i32,
) {
    if vim.ux_browser_id() != Some(bid) {
        return;
    }
    let Some(wid) = crate::browser::cef::osr::window_id_for_browser(
        crate::runtime::ffi_osr_index().as_ref(),
        bid,
    ) else {
        vim.exit_ux_to_browse();
        vim.find_committed.clear();
        return;
    };
    cleanup_vimium_ux_ui_at_window(osr_host, vim, wid);
    vim.exit_ux_to_browse();
    vim.find_committed.clear();
    osr_host.nudge_osr_view_after_input(wid);
}

pub(crate) fn handle_find_mode_pressed(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    window_id: WindowId,
    event: &winit::event::KeyEvent,
) -> bool {
    queue_find_mode_key(osr_host, rt, window_id, event.clone());
    true
}

pub(crate) fn handle_find_mode_pressed_inner(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
    window_id: WindowId,
    event: &winit::event::KeyEvent,
) -> bool {
    let ctrl = rt.mods_winit.control_key();
    let cmd = rt.mods_winit.super_key();
    if ctrl || cmd {
        return false;
    }
    let Some(browser) = osr_host.browser_for_window(window_id) else {
        return true;
    };
    if event.state == ElementState::Released {
        return true;
    }

    match &event.logical_key {
        Key::Named(NamedKey::Escape) => {
            modes::find_ui_hide(&browser);
            modes::cef_stop_finding(&browser, true);
            vim.cancel_find();
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
        Key::Named(NamedKey::Enter) => {
            modes::find_ui_hide(&browser);
            vim.finish_find_accept();
            if vim.find_committed.is_empty() {
                modes::cef_stop_finding(&browser, true);
            }
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
        Key::Named(NamedKey::Backspace) => {
            let Some(query): Option<&mut String> = vim.find_query_mut() else {
                return true;
            };
            query.pop();
            modes::find_ui_set_query(&browser, query);
            modes::cef_find(&browser, query, true, false);
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
        Key::Character(input) => {
            let Some(query): Option<&mut String> = vim.find_query_mut() else {
                return true;
            };
            if input.chars().any(|c| c.is_control()) {
                return true;
            }
            query.push_str(input);
            modes::find_ui_set_query(&browser, query);
            modes::cef_find(&browser, query, true, false);
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
        _ => {}
    }
    let Some(query): Option<&mut String> = vim.find_query_mut() else {
        return true;
    };
    // Fallback for platforms/keys where logical key did not provide a character.
    if let Some(t) = &event.text {
        for ch in t.chars() {
            if !ch.is_control() {
                query.push(ch);
            }
        }
        modes::find_ui_set_query(&browser, query);
        modes::cef_find(&browser, query, true, false);
        osr_host.nudge_osr_view_after_input(window_id);
        return true;
    }
    // Physical letters: macOS can deliver `LogicalKey::Unidentified` and empty `text` for a
    // printable key (IME / pipeline timing), so find-in-page would eat the stroke with no effect.
    if let Some(ch) = keyboard::lowercase_letter_from_physical(&event.physical_key) {
        query.push(ch);
        modes::find_ui_set_query(&browser, query);
        modes::cef_find(&browser, query, true, false);
        osr_host.nudge_osr_view_after_input(window_id);
        return true;
    }
    true
}
fn try_handle_vimium_keys_traced(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
    window_id: WindowId,
    event: &winit::event::KeyEvent,
    ecs_browser: Option<(Entity, i32)>,
    editable_focus: &EditableFocusSnapshot,
    out: &mut BrowserEventBatch,
) -> bool {
    let handled = try_handle_vimium_keys_inner(
        osr_host,
        rt,
        vim,
        window_id,
        event,
        false,
        ecs_browser,
        editable_focus,
        out,
    );
    if shell_input::shell_input_trace_enabled() {
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            ?window_id,
            vimium_handled = handled,
            state = ?event.state,
            repeat = event.repeat,
            physical = ?event.physical_key,
            logical = ?event.logical_key,
            text = ?event.text,
            "shell_input: KeyboardInput (after try_handle_vimium_keys)",
        );
    }
    handled
}

pub(crate) fn try_handle_vimium_keys_after_editable_probe(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
    window_id: WindowId,
    event: &winit::event::KeyEvent,
    ecs_browser: Option<(Entity, i32)>,
    editable_focus: &EditableFocusSnapshot,
    out: &mut BrowserEventBatch,
) -> bool {
    try_handle_vimium_keys_inner(
        osr_host,
        rt,
        vim,
        window_id,
        event,
        true,
        ecs_browser,
        editable_focus,
        out,
    )
}

/// macOS: focused web nodes (e.g. Google consent language control) often emit `Ime::Commit` without
/// a matching `KeyboardInput`, so vimium never sees the key. Mirror
/// [`try_handle_vimium_keys_inner`] for **single-character** commits representable via
/// [`chord_matches_winit_ime_char`]. Caller must pass a **resolved** `browser_id` and must **not**
/// hold `windows_store` locked (see `nudge_osr_view_after_input`).
fn try_handle_vimium_ime_commit_traced(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
    window_id: WindowId,
    browser_id: i32,
    text: &str,
    editable_focus: &EditableFocusSnapshot,
    out: &mut BrowserEventBatch,
) -> bool {
    let handled = try_handle_vimium_ime_commit_inner(
        osr_host,
        rt,
        vim,
        window_id,
        browser_id,
        text,
        editable_focus,
        out,
    );
    if shell_input::shell_input_trace_enabled() {
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            browser_id,
            vimium_handled = handled,
            text = ?text,
            "shell_input: Ime::Commit (after try_handle_vimium_ime_commit)",
        );
    }
    handled
}

fn try_handle_vimium_ime_commit_inner(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
    window_id: WindowId,
    browser_id: i32,
    text: &str,
    editable_focus: &EditableFocusSnapshot,
    out: &mut BrowserEventBatch,
) -> bool {
    let km = &*osr_host.key_settings;
    let bid = browser_id;
    if !km.enabled {
        if shell_input::shell_input_trace_enabled() {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                bid,
                "shell_input: ime_commit skipped (vimium disabled in settings)",
            );
        }
        return false;
    }

    let text = text.trim();
    let mut chars = text.chars();
    let Some(c0) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        if shell_input::shell_input_trace_enabled() {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                bid,
                trimmed = ?text,
                "shell_input: ime_commit rejected (multi-codepoint after trim; single-char IME path only)",
            );
        }
        return false;
    }

    let now = Instant::now();

    if let Some(hid_bid) = vim.expire_link_hints_if_due(now) {
        out.link_hints_hide.push(LinkHintsHideBrowserEvent {
            browser_id: hid_bid,
        });
        osr_host.nudge_osr_view_after_input(window_id);
    }

    let mods = rt.mods_winit;
    let mod_ctrl = mods.control_key();
    let mod_cmd = mods.super_key();
    let mod_alt = mods.alt_key();
    let no_ctrl_alt_cmd = !mod_ctrl && !mod_alt && !mod_cmd;

    // Match `try_handle_vimium_keys_inner` link-hints UX: dismiss on non-hint keys / IME Esc;
    // feed plain ASCII alphanumerics as hint characters (labels can use digits).
    if vim.link_hints_active() && vim.link_hints_browser_id() == Some(bid) {
        let is_hint_letter = c0.is_ascii_alphanumeric();
        let esc = c0 == '\u{1b}';

        if !(is_hint_letter && no_ctrl_alt_cmd) {
            if !editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                out.link_hints_hide
                    .push(LinkHintsHideBrowserEvent { browser_id: bid });
                vim.clear_link_hints();
                osr_host.nudge_osr_view_after_input(window_id);
            }
        }

        if !vim.link_hints_active() {
            return false;
        }

        if esc && no_ctrl_alt_cmd {
            out.link_hints_hide
                .push(LinkHintsHideBrowserEvent { browser_id: bid });
            vim.clear_link_hints();
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }

        if is_hint_letter && no_ctrl_alt_cmd {
            let ch = c0.to_ascii_lowercase();
            let prior = vim.link_hints_typed_prefix().len();
            out.link_hints_feed_key_deferred
                .push(LinkHintsFeedKeyDeferredBrowserEvent {
                    browser_id: bid,
                    ch,
                    prior_typed_len: prior,
                });
            return true;
        }

        return false;
    }

    let window_ms = km.scroll_top_double_press_ms;
    if let Some(prev) = vim.scroll_g_pending {
        if now.duration_since(prev) > Duration::from_millis(window_ms) {
            vim.scroll_g_pending = None;
        }
    }

    if vim.is_insert(bid) || vim.is_find(bid) || vim.is_visual(bid) {
        return false;
    }

    if c0 == '\u{1b}' && no_ctrl_alt_cmd {
        blur_trapped_dom_focus_browse_mode(osr_host, window_id, bid, vim);
        return false;
    }

    if let Some(ref chord) = km.hint_links {
        if chord_matches_winit_ime_char(chord, mods, c0) {
            out.link_hints_show
                .push(LinkHintsShowBrowserEvent { browser_id: bid });
            vim.arm_link_hints(bid, now);
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
    }

    if let Some(ref chord) = km.history_back {
        if chord_matches_winit_ime_char(chord, mods, c0) {
            if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                vim.scroll_g_pending = None;
                osr_host.request_set_active_browser(bid);
                out.navigate.push(NavigateBrowserEvent {
                    browser_id: bid,
                    go_forward: false,
                });
                return true;
            }
            return false;
        }
    }
    if let Some(ref chord) = km.history_forward {
        if chord_matches_winit_ime_char(chord, mods, c0) {
            if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                vim.scroll_g_pending = None;
                osr_host.request_set_active_browser(bid);
                out.navigate.push(NavigateBrowserEvent {
                    browser_id: bid,
                    go_forward: true,
                });
                return true;
            }
            return false;
        }
    }

    if let Some(browser) = browser_for_vimium_target(osr_host, window_id, bid) {
        if !vim.find_committed.is_empty() {
            if let Some(ref chord) = km.find_next {
                if chord_matches_winit_ime_char(chord, mods, c0) {
                    if editable_gating::editable_focus_is_typing(editable_focus, bid) {
                        return false;
                    }
                    vim.scroll_g_pending = None;
                    modes::cef_find(&browser, &vim.find_committed, true, true);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.find_prev {
                if chord_matches_winit_ime_char(chord, mods, c0) {
                    if editable_gating::editable_focus_is_typing(editable_focus, bid) {
                        return false;
                    }
                    vim.scroll_g_pending = None;
                    modes::cef_find(&browser, &vim.find_committed, false, true);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
        }
    }

    let might_mode_chord = [
        km.mode_insert.as_ref(),
        km.mode_find_open.as_ref(),
        km.mode_visual.as_ref(),
        km.yank_url.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|c| chord_matches_winit_ime_char(c, mods, c0));

    if might_mode_chord {
        let page_ok_modes = true;
        if let Some(browser) = browser_for_vimium_target(osr_host, window_id, bid) {
            if let Some(ref chord) = km.mode_insert {
                if chord_matches_winit_ime_char(chord, mods, c0) && page_ok_modes {
                    clear_vimium_ux_if_other_browser(osr_host, vim, bid);
                    teardown_vimium_ux_at_window_keep_committed(osr_host, vim, window_id, bid);
                    vim.enter_insert(bid);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.mode_find_open {
                if chord_matches_winit_ime_char(chord, mods, c0) && page_ok_modes {
                    clear_vimium_ux_if_other_browser(osr_host, vim, bid);
                    teardown_vimium_ux_at_window_keep_committed(osr_host, vim, window_id, bid);
                    vim.enter_find(bid);
                    modes::find_ui_show(&browser);
                    modes::find_ui_set_query(&browser, "");
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.mode_visual {
                if chord_matches_winit_ime_char(chord, mods, c0)
                    && page_ok_modes
                    && !rt.primary_mouse_down
                    && !vim.link_hints_active()
                {
                    clear_vimium_ux_if_other_browser(osr_host, vim, bid);
                    teardown_vimium_ux_at_window_keep_committed(osr_host, vim, window_id, bid);
                    vim.enter_visual(bid);
                    modes::visual_hint_show(&browser);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.yank_url {
                if chord_matches_winit_ime_char(chord, mods, c0) && page_ok_modes {
                    vim.scroll_g_pending = None;
                    modes::yank_page_url(&browser);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
        }
    }

    let pass_scroll_to_page = || editable_gating::editable_focus_is_typing(editable_focus, bid);

    if let Some(browser) = browser_for_vimium_target(osr_host, window_id, bid) {
        if let Some(ref chord) = km.scroll_line_down {
            if chord_matches_winit_ime_char(chord, mods, c0) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_line_down(&browser, false);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_line_up {
            if chord_matches_winit_ime_char(chord, mods, c0) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_line_up(&browser, false);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_page_down {
            if chord_matches_winit_ime_char(chord, mods, c0) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_page_down(&browser, false);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_page_up {
            if chord_matches_winit_ime_char(chord, mods, c0) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_page_up(&browser, false);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_bottom {
            if chord_matches_winit_ime_char(chord, mods, c0) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_bottom(&browser);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.reload {
            if chord_matches_winit_ime_char(chord, mods, c0) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                osr_host.request_set_active_browser(bid);
                out.reload.push(ReloadBrowserEvent { browser_id: bid });
                return true;
            }
        }

        if let Some(ref prefix) = km.scroll_top_prefix {
            if chord_matches_winit_ime_char(prefix, mods, c0) {
                if pass_scroll_to_page() {
                    vim.scroll_g_pending = None;
                    return false;
                }
                if let Some(prev) = vim.scroll_g_pending {
                    if now.duration_since(prev) <= Duration::from_millis(window_ms) {
                        vim.scroll_g_pending = None;
                        scroll::scroll_top(&browser);
                        osr_host.nudge_osr_view_after_input(window_id);
                        return true;
                    }
                }
                vim.scroll_g_pending = Some(now);
                return true;
            }
        }
    }

    let prefix_matches = km
        .scroll_top_prefix
        .as_ref()
        .is_some_and(|p| chord_matches_winit_ime_char(p, mods, c0));
    if !prefix_matches {
        vim.scroll_g_pending = None;
    }

    if shell_input::shell_input_trace_enabled() {
        let browser_resolved = browser_for_vimium_target(osr_host, window_id, bid).is_some();
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            bid,
            char = ?c0,
            browser_resolved,
            shift = mods.shift_key(),
            ctrl = mods.control_key(),
            alt = mods.alt_key(),
            cmd = mods.super_key(),
            may_history_shortcuts = editable_gating::may_handle_history_shortcuts(editable_focus, bid),
            editable_probe_typing = editable_gating::editable_focus_is_typing(editable_focus, bid),
            "shell_input: ime_commit no matcher (will fall through to send_char unless swallowed)",
        );
    }

    false
}

/// `editable_hint_fresh`: editable-focus DOM probe has just run; skip scheduling another probe before branching.
pub(crate) fn try_handle_vimium_keys_inner(
    osr_host: &mut OsrHostState,
    rt: &mut RuntimeState,
    vim: &mut VimiumState,
    window_id: WindowId,
    event: &winit::event::KeyEvent,
    editable_hint_fresh: bool,
    ecs_browser: Option<(Entity, i32)>,
    editable_focus: &EditableFocusSnapshot,
    out: &mut BrowserEventBatch,
) -> bool {
    let km = Arc::clone(&osr_host.key_settings);
    let Some(bid) = osr_host.browser_id_for_window(window_id, ecs_browser) else {
        return false;
    };

    if !km.enabled {
        if vim.link_hints_browser_id() == Some(bid) {
            out.link_hints_hide
                .push(LinkHintsHideBrowserEvent { browser_id: bid });
            osr_host.nudge_osr_view_after_input(window_id);
            vim.clear_link_hints();
        }
        if vim.ux_browser_id() == Some(bid) {
            cleanup_vimium_ux_ui_at_window(osr_host, vim, window_id);
            vim.exit_ux_to_browse();
            vim.find_committed.clear();
        }
        return false;
    }

    if vim.find_swallows_keyup(bid) {
        if event.state == ElementState::Released {
            return true;
        }
    }

    if event.state != ElementState::Pressed {
        return false;
    }

    let chord_mods = rt.mods_winit;
    let physical = &event.physical_key;
    let mod_shift = chord_mods.shift_key();
    let mod_ctrl = chord_mods.control_key();
    let mod_alt = chord_mods.alt_key();
    let mod_cmd = chord_mods.super_key();
    let key_sends_printable_text = keyboard::keyevent_has_printable_text(event);
    let now = Instant::now();

    if let Some(hid_bid) = vim.expire_link_hints_if_due(now) {
        out.link_hints_hide.push(LinkHintsHideBrowserEvent {
            browser_id: hid_bid,
        });
        osr_host.nudge_osr_view_after_input(window_id);
    }

    // `LinkHints`: when typing the hint letters themselves we must NOT dismiss based on
    // "editable focus" probes, otherwise hints can disappear without activating a target.
    if vim.link_hints_active() {
        let is_hint_letter = keyboard::hint_label_char_from_key_event(event).is_some();
        let esc = match physical {
            PhysicalKey::Code(KeyCode::Escape) => true,
            _ => false,
        };
        let no_ctrl_alt_cmd = !mod_ctrl && !mod_alt && !mod_cmd;

        // Dismiss only for keys other than plain hint letters.
        if !(is_hint_letter && no_ctrl_alt_cmd) {
            if !editable_hint_fresh {
                return defer_vimium_key_after_editable_probe(osr_host, window_id, bid, event);
            }
            if !editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                out.link_hints_hide
                    .push(LinkHintsHideBrowserEvent { browser_id: bid });
                vim.clear_link_hints();
                osr_host.nudge_osr_view_after_input(window_id);
            }
        }

        if !vim.link_hints_active() {
            // Dismissed above.
            return false;
        }

        if esc && no_ctrl_alt_cmd {
            out.link_hints_hide
                .push(LinkHintsHideBrowserEvent { browser_id: bid });
            vim.clear_link_hints();
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
        return false;
    }

    let window_ms = km.scroll_top_double_press_ms;
    if let Some(prev) = vim.scroll_g_pending {
        if now.duration_since(prev) > Duration::from_millis(window_ms) {
            vim.scroll_g_pending = None;
        }
    }

    if vim.is_insert(bid) {
        let esc = match physical {
            PhysicalKey::Code(KeyCode::Escape) => true,
            _ => false,
        };
        let no_mod = !mod_ctrl && !mod_cmd && !mod_alt && !mod_shift;
        let ctrl_ob = mod_ctrl
            && !mod_cmd
            && !mod_alt
            && match physical {
                PhysicalKey::Code(KeyCode::BracketLeft) => true,
                _ => false,
            };
        if (esc && no_mod) || ctrl_ob {
            vim.exit_ux_to_browse();
            return true;
        }
        return false;
    }

    if vim.is_find(bid) {
        return handle_find_mode_pressed(osr_host, rt, window_id, event);
    }

    if vim.is_visual(bid) {
        let esc = match physical {
            PhysicalKey::Code(KeyCode::Escape) => true,
            _ => false,
        };
        let no_mod = !mod_ctrl && !mod_cmd && !mod_alt && !mod_shift;
        if esc && no_mod {
            if let Some(b) = browser_for_vimium_target(osr_host, window_id, bid) {
                modes::visual_hint_hide(&b);
            }
            vim.exit_ux_to_browse();
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
        let y_plain = !mod_shift && !mod_ctrl && !mod_cmd && !mod_alt;
        if y_plain {
            match physical {
                PhysicalKey::Code(KeyCode::KeyY) => {
                    if let Some(b) = browser_for_vimium_target(osr_host, window_id, bid) {
                        modes::yank_selection(&b);
                        osr_host.nudge_osr_view_after_input(window_id);
                    }
                    return true;
                }
                _ => {}
            }
        }
        return false;
    }

    // Browse: blur trapped consent / language focus so Esc can dismiss UI and keys aren't stuck on FR.
    // Still return false so CEF receives the Escape key event as well.
    if matches!(physical, PhysicalKey::Code(KeyCode::Escape)) && !mod_ctrl && !mod_alt && !mod_cmd {
        blur_trapped_dom_focus_browse_mode(osr_host, window_id, bid, vim);
    }

    // Link hints (`f`): before the printable-text bail (winit sets `text` on letter keys).
    //
    // Do **not** gate on editable-focus the way scroll keys do: home pages (e.g. Google) often
    // autofocus the search `<input>` and the DOM probe reports `Some(true)`, which would block `f`
    // forever. Arming hints steals `f` from the page until Esc — same trade-off as real Vimium
    // when you invoke hints from a focused field.
    //
    // Do **not** defer through the editable-probe + `ShortcutKeyReplayEvent` path: that async chain
    // races the first paint (frames/browser handles not ready, hint-show running before replay).
    // `f` does not need a fresh probe for routing — show/hide and JS overlay stand alone.
    if let Some(ref chord) = km.hint_links {
        if chord_matches_winit_or_text(
            chord,
            chord_mods,
            physical,
            event.text.as_deref(),
            &event.logical_key,
        ) {
            out.link_hints_show
                .push(LinkHintsShowBrowserEvent { browser_id: bid });
            vim.arm_link_hints(bid, now);
            osr_host.nudge_osr_view_after_input(window_id);
            return true;
        }
    }

    // Scroll / reload: when the DOM probe says we're in a text control **or** winit attached
    // printable `text` to this keydown, pass the key to CEF so words like "ledger" are not reduced
    // to "lee" (`d`/`g`/`r` bindings). Trade-off: with autofocused search and a **false** "not
    // typing" probe, j/k may still scroll until the probe catches up — use **insert mode** (`i`) if
    // needed.

    // History: use `may_handle_history_shortcuts` (block only when probe is **sure** we're in a text
    // control), not `page_allows_app_shortcuts` — otherwise Shift+H/L never run until the probe
    // reports explicit “not editable”, unlike Cmd+[ / Alt+←→ in `window::dispatch`.
    if let Some(ref chord) = km.history_back {
        if chord_matches_winit(chord, chord_mods, physical) {
            if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                vim.scroll_g_pending = None;
                osr_host.request_set_active_browser(bid);
                out.navigate.push(NavigateBrowserEvent {
                    browser_id: bid,
                    go_forward: false,
                });
                return true;
            }
            return false;
        }
    }
    if let Some(ref chord) = km.history_forward {
        if chord_matches_winit(chord, chord_mods, physical) {
            if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                vim.scroll_g_pending = None;
                osr_host.request_set_active_browser(bid);
                out.navigate.push(NavigateBrowserEvent {
                    browser_id: bid,
                    go_forward: true,
                });
                return true;
            }
            return false;
        }
    }

    if let Some(browser) = browser_for_vimium_target(osr_host, window_id, bid) {
        if !vim.find_committed.is_empty() {
            if let Some(ref chord) = km.find_next {
                if chord_matches_winit(chord, chord_mods, physical) {
                    // Match IME path + history shortcuts: only yield when the probe is **sure** we're
                    // in a text control — not `pass_letter_like` (unknown probe + printable `text`
                    // would send Shift+N to Google instead of find-next).
                    if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                        vim.scroll_g_pending = None;
                        modes::cef_find(&browser, &vim.find_committed, true, true);
                        osr_host.nudge_osr_view_after_input(window_id);
                        return true;
                    }
                    return false;
                }
            }
            if let Some(ref chord) = km.find_prev {
                if chord_matches_winit(chord, chord_mods, physical) {
                    if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                        vim.scroll_g_pending = None;
                        modes::cef_find(&browser, &vim.find_committed, false, true);
                        osr_host.nudge_osr_view_after_input(window_id);
                        return true;
                    }
                    return false;
                }
            }
        }
    }

    // Avoid enqueueing editable-focus probes on every key: each runs a DOM visit +
    // message-loop pump and can crash or corrupt CEF when re-entered while typing in an `<input>`.
    let might_mode_chord = [
        km.mode_insert.as_ref(),
        km.mode_find_open.as_ref(),
        km.mode_visual.as_ref(),
        km.yank_url.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|c| chord_matches_winit(c, chord_mods, physical));

    if might_mode_chord {
        // Omit `page_allows_app_shortcuts` (Google autofocus) and empty-`text` IME heuristics so
        // `/` / `i` / … work on first paint (see module comment above).
        let page_ok_modes = true;

        if let Some(browser) = browser_for_vimium_target(osr_host, window_id, bid) {
            if let Some(ref chord) = km.mode_insert {
                if chord_matches_winit(chord, chord_mods, physical) && page_ok_modes {
                    clear_vimium_ux_if_other_browser(osr_host, vim, bid);
                    teardown_vimium_ux_at_window_keep_committed(osr_host, vim, window_id, bid);
                    vim.enter_insert(bid);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.mode_find_open {
                if chord_matches_winit(chord, chord_mods, physical) && page_ok_modes {
                    clear_vimium_ux_if_other_browser(osr_host, vim, bid);
                    teardown_vimium_ux_at_window_keep_committed(osr_host, vim, window_id, bid);
                    vim.enter_find(bid);
                    modes::find_ui_show(&browser);
                    modes::find_ui_set_query(&browser, "");
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.mode_visual {
                if chord_matches_winit(chord, chord_mods, physical)
                    && page_ok_modes
                    && !rt.primary_mouse_down
                    && !vim.link_hints_active()
                {
                    clear_vimium_ux_if_other_browser(osr_host, vim, bid);
                    teardown_vimium_ux_at_window_keep_committed(osr_host, vim, window_id, bid);
                    vim.enter_visual(bid);
                    modes::visual_hint_show(&browser);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
            if let Some(ref chord) = km.yank_url {
                if chord_matches_winit(chord, chord_mods, physical) && page_ok_modes {
                    vim.scroll_g_pending = None;
                    modes::yank_page_url(&browser);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
            }
        }
    }

    let pass_scroll_to_page = || {
        editable_gating::pass_letter_like_vimium_binding_to_page(
            editable_focus,
            bid,
            key_sends_printable_text,
        )
    };

    if let Some(browser) = browser_for_vimium_target(osr_host, window_id, bid) {
        if let Some(ref chord) = km.scroll_line_down {
            if chord_matches_winit(chord, chord_mods, physical) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_line_down(&browser, event.repeat);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_line_up {
            if chord_matches_winit(chord, chord_mods, physical) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_line_up(&browser, event.repeat);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_page_down {
            if chord_matches_winit(chord, chord_mods, physical) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_page_down(&browser, event.repeat);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_page_up {
            if chord_matches_winit(chord, chord_mods, physical) {
                if pass_scroll_to_page() {
                    return false;
                }
                vim.scroll_g_pending = None;
                scroll::scroll_page_up(&browser, event.repeat);
                osr_host.nudge_osr_view_after_input(window_id);
                return true;
            }
        }
        if let Some(ref chord) = km.scroll_bottom {
            if chord_matches_winit(chord, chord_mods, physical) {
                // Shift+G: same gate as Shift+H/L / reload — do not use `pass_letter_like`, or a
                // capital `G` in `event.text` can leak to the page before the editable probe settles.
                if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                    vim.scroll_g_pending = None;
                    scroll::scroll_bottom(&browser);
                    osr_host.nudge_osr_view_after_input(window_id);
                    return true;
                }
                return false;
            }
        }
        if let Some(ref chord) = km.reload {
            if chord_matches_winit(chord, chord_mods, physical) {
                if editable_gating::may_handle_history_shortcuts(editable_focus, bid) {
                    vim.scroll_g_pending = None;
                    osr_host.request_set_active_browser(bid);
                    out.reload.push(ReloadBrowserEvent { browser_id: bid });
                    return true;
                }
                return false;
            }
        }

        if let Some(ref prefix) = km.scroll_top_prefix {
            if chord_matches_winit(prefix, chord_mods, physical) {
                if pass_scroll_to_page() {
                    vim.scroll_g_pending = None;
                    return false;
                }
                if let Some(prev) = vim.scroll_g_pending {
                    if now.duration_since(prev) <= Duration::from_millis(window_ms) {
                        vim.scroll_g_pending = None;
                        scroll::scroll_top(&browser);
                        osr_host.nudge_osr_view_after_input(window_id);
                        return true;
                    }
                }
                vim.scroll_g_pending = Some(now);
                return true;
            }
        }
    }

    let prefix_matches = km
        .scroll_top_prefix
        .as_ref()
        .is_some_and(|p| chord_matches_winit(p, chord_mods, physical));
    if !prefix_matches {
        vim.scroll_g_pending = None;
    }

    false
}

impl OsrKeyboardShellSnapshot for VimiumState {
    fn link_hints_active(&self) -> bool {
        VimiumState::link_hints_active(self)
    }

    fn link_hints_browser_id(&self) -> Option<i32> {
        VimiumState::link_hints_browser_id(self)
    }

    fn link_hints_typed_prefix_len(&self) -> usize {
        VimiumState::link_hints_typed_prefix(self).len()
    }
}

impl ShellKeyboardHandler for VimiumState {
    fn apply_link_hints_navigation_resets(
        &mut self,
        osr_host: &mut OsrHostState,
        out: &mut BrowserEventBatch,
        browser_ids: Vec<i32>,
    ) {
        apply_link_hints_navigation_resets_impl(osr_host, self, out, browser_ids);
    }

    fn try_handle_vimium_keys(
        &mut self,
        osr_host: &mut OsrHostState,
        rt: &mut RuntimeState,
        window_id: WindowId,
        event: &KeyEvent,
        ecs_browser: Option<(Entity, i32)>,
        editable_focus: &EditableFocusSnapshot,
        out: &mut BrowserEventBatch,
    ) -> bool {
        try_handle_vimium_keys_traced(
            osr_host,
            rt,
            self,
            window_id,
            event,
            ecs_browser,
            editable_focus,
            out,
        )
    }

    fn try_handle_vimium_ime_commit(
        &mut self,
        osr_host: &mut OsrHostState,
        rt: &mut RuntimeState,
        window_id: WindowId,
        browser_id: i32,
        text: &str,
        editable_focus: &EditableFocusSnapshot,
        out: &mut BrowserEventBatch,
    ) -> bool {
        try_handle_vimium_ime_commit_traced(
            osr_host,
            rt,
            self,
            window_id,
            browser_id,
            text,
            editable_focus,
            out,
        )
    }
}
