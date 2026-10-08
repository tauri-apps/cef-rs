//! Shell keyboard / IME bridge: traits and tracing so [`crate::window`] does not import [`crate::vimium`].
//!
//! Manual WM keys are emitted as [`crate::tmux::event::TmuxWmShellKeyboardEvent`] and processed in
//! [`crate::tmux::system::process_tmux_wm_shell_keyboard_events_system`] (before Vimium / CEF forward
//! for those keys). Other shell routing uses [`crate::window::dispatch::handle_window_event_inner`]
//! after [`crate::window::dispatch::window_dispatch_prelude`].
//! The [`ShellKeyboardHandler`] trait keeps window/tmux on the `input` boundary (no `vimium` imports there).
//!
//! [`VimiumState`](crate::vimium::state::VimiumState) implements [`ShellKeyboardHandler`] in `vimium/window_input.rs`.

use bevy_ecs::entity::Entity;
use winit::event::{ElementState, Ime, KeyEvent};
use winit::window::WindowId;

use crate::browser::cef::facet::EditableFocusSnapshot;
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::BrowserEventBatch;
use crate::runtime::RuntimeState;

#[inline]
pub fn shell_input_trace_enabled() -> bool {
    crate::log::shell_input_trace_enabled()
}

/// Snapshot for CEF keyboard forward (link-hints chip state).
pub trait OsrKeyboardShellSnapshot {
    fn link_hints_active(&self) -> bool;
    fn link_hints_browser_id(&self) -> Option<i32>;
    fn link_hints_typed_prefix_len(&self) -> usize;
}

/// Shell key / IME handling invoked from the window dispatch path ([`crate::window::dispatch::handle_window_event_inner`], tmux system).
pub trait ShellKeyboardHandler: OsrKeyboardShellSnapshot {
    fn apply_link_hints_navigation_resets(
        &mut self,
        osr_host: &mut OsrHostState,
        out: &mut BrowserEventBatch,
        browser_ids: Vec<i32>,
    );

    fn try_handle_vimium_keys(
        &mut self,
        osr_host: &mut OsrHostState,
        rt: &mut RuntimeState,
        window_id: WindowId,
        event: &KeyEvent,
        ecs_browser: Option<(Entity, i32)>,
        editable_focus: &EditableFocusSnapshot,
        out: &mut BrowserEventBatch,
    ) -> bool;

    fn try_handle_vimium_ime_commit(
        &mut self,
        osr_host: &mut OsrHostState,
        rt: &mut RuntimeState,
        window_id: WindowId,
        browser_id: i32,
        text: &str,
        editable_focus: &EditableFocusSnapshot,
        out: &mut BrowserEventBatch,
    ) -> bool;
}

pub fn trace_winit_ime_event(window_id: WindowId, browser_id: i32, ime: &Ime) {
    if !shell_input_trace_enabled() {
        return;
    }
    match ime {
        Ime::Commit(s) => {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                ?window_id,
                browser_id,
                commit_len = s.len(),
                commit = ?s,
                "shell_input: Ime::Commit (before shell IME commit handler)",
            );
        }
        Ime::Preedit(s, caret) => {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                ?window_id,
                browser_id,
                preedit_len = s.len(),
                preedit = ?s,
                caret = ?caret,
                "shell_input: Ime::Preedit",
            );
        }
        Ime::Disabled => {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                ?window_id,
                browser_id,
                "shell_input: Ime::Disabled",
            );
        }
        Ime::Enabled => {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                ?window_id,
                browser_id,
                "shell_input: Ime::Enabled",
            );
        }
    }
}

pub fn trace_cef_keydown_forward_from_winit(browser_id: i32, event: &KeyEvent, vk: i32) {
    if !shell_input_trace_enabled() || event.state != ElementState::Pressed {
        return;
    }
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        browser_id,
        vk,
        physical = ?event.physical_key,
        "shell_input: KeyboardInput → CEF RAWKEYDOWN/KEYDOWN",
    );
}

pub fn trace_cef_send_char_forward_from_winit(browser_id: i32, text: &str) {
    if !shell_input_trace_enabled() {
        return;
    }
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        browser_id,
        text = ?text,
        "shell_input: KeyboardInput → send_char to CEF",
    );
}

pub fn trace_ime_commit_forward_to_cef(browser_id: i32, text: &str) {
    if !shell_input_trace_enabled() {
        return;
    }
    bevy_log::info!(
        target: "vmux",
        pid = std::process::id(),
        browser_id,
        text = ?text,
        "shell_input: Ime::Commit → send_char to CEF (shell handler did not consume commit)",
    );
}
