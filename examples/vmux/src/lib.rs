//! vmux: CEF off-screen shell, Bevy ECS, and compositor integration.
//!
//! **Platform:** macOS only (Windows/Linux support is intentionally removed for now).
//!
//! The library is built as **`cdylib`** for bundle / helper layouts that expect a dynamic
//! library artifact; **`rlib`** is included so the `vmux` binary and tests link the same crate graph.
//!
//! Primary embed entry point: [`VmuxPlugin`] (registers settings, [`runtime::RuntimePlugin`], browser, windowing, tmux, input, vimium).

// Scaffold / CEF glue triggers many `dead_code` and clippy findings; tighten per-module once call sites land.
#![allow(dead_code)]
#![allow(clippy::all)]

#[cfg(not(target_os = "macos"))]
compile_error!("vmux is supported on macOS only");

use bevy_app::{App, Plugin};

pub mod browser;
pub mod input;
pub mod log;
pub mod runtime;
pub mod settings;
pub mod tmux;
mod vimium;
pub mod window;

pub use input::InputPlugin;
pub use tmux::TmuxPlugin;
pub use vimium::VimiumPlugin;

/// Registers settings, winit-backed [`runtime::RuntimePlugin`], browser (CEF OSR), windowing, tmux, input, and vimium.
pub struct VmuxPlugin;

impl VmuxPlugin {
    pub const fn new() -> Self {
        Self
    }
}

impl Plugin for VmuxPlugin {
    fn build(&self, app: &mut App) {
        // `RuntimePlugin` (custom winit runner + `RuntimeState`) before `BrowserPlugin` / `WindowsPlugin` so shell dispatch sees runtime resources.
        // `BrowserPlugin` before `TmuxPlugin` / `WindowsPlugin` so Bevy events used by window ingest exist at build time.
        // `TmuxPlugin` before `WindowsPlugin` so `TmuxWmShellKeyboardEvent` is registered before window systems use `EventWriter` for it.
        // `TmuxPlugin` registers `process_tmux_wm_shell_keyboard_events_system` after window apply; `BrowserPlugin` registers `fanout_browser_shell_effect_batch_events_system` after that.
        // Chord sources: winit path in `window::dispatch` when `WmMode::Manual`.
        // `InputPlugin` after browser so `apply_browser_ui_ops_system` exists for `InputSystems`.
        // `ShellKeyboardStateResource` / vimium state is registered by `VimiumPlugin`; vimium systems run after `apply_browser_ui_ops_system`.
        app.add_plugins(crate::settings::SettingsPlugin)
            .add_plugins(crate::runtime::RuntimePlugin::new())
            .add_plugins(crate::browser::BrowserPlugin::new())
            .add_plugins(crate::tmux::TmuxPlugin)
            .add_plugins(crate::window::WindowsPlugin)
            .add_plugins(crate::input::InputPlugin)
            .add_plugins(crate::vimium::VimiumPlugin);
    }
}

#[cfg(test)]
use bevy_ecs::event::EventWriter;
#[cfg(test)]
use bevy_ecs::prelude::Res;
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
use crate::browser::event::ShortcutKeyReplayEvent;
#[cfg(test)]
use crate::input::system::EditableFocusQueues;
#[cfg(test)]
use crate::input::ShellKeyboardStateResource;
#[cfg(test)]
use crate::runtime::RuntimePlugin;
#[cfg(test)]
use crate::window::event::OsrHostWindowDispatch;

#[cfg(test)]
fn vimium_state_accessible_system(_: Res<ShellKeyboardStateResource>) {}

#[cfg(test)]
#[test]
fn vmux_plugin_registers_vimium_plugin_via_system() {
    let mut app = App::new();
    app.add_plugins(VmuxPlugin::new());
    assert!(
        app.is_plugin_added::<VimiumPlugin>(),
        "deferred vimium keys post-probe are replayed only by VimiumPlugin systems"
    );
    app.world_mut()
        .run_system_once(vimium_state_accessible_system)
        .expect("ShellKeyboardStateResource (vimium) must be registered by VimiumPlugin, not runtime");
}

#[cfg(test)]
fn osr_host_dispatch_writer_system(mut _w: EventWriter<OsrHostWindowDispatch>) {}

#[cfg(test)]
#[test]
fn vmux_registers_osr_host_window_dispatch_event_via_system() {
    let mut app = App::new();
    app.add_plugins(VmuxPlugin::new());
    app.world_mut()
        .run_system_once(osr_host_dispatch_writer_system)
        .expect("WindowsPlugin must register OsrHostWindowDispatch for winit→Bevy shell dispatch");
}

#[cfg(test)]
fn shortcut_replay_writer_system(mut _w: EventWriter<ShortcutKeyReplayEvent>) {}

#[cfg(test)]
fn editable_focus_queues_accessible_system(_: Res<EditableFocusQueues>) {}

#[cfg(test)]
#[test]
fn vmux_plugin_registers_input_plugin_and_shortcut_replay_event_via_system() {
    let mut app = App::new();
    app.add_plugins(VmuxPlugin::new());
    assert!(app.is_plugin_added::<InputPlugin>());
    app.world_mut()
        .run_system_once(shortcut_replay_writer_system)
        .expect("InputPlugin must register ShortcutKeyReplayEvent");
    app.world_mut()
        .run_system_once(editable_focus_queues_accessible_system)
        .expect("InputPlugin must register EditableFocusQueues");
}

#[cfg(test)]
#[test]
fn vmux_plugin_registers_tmux_plugin() {
    let mut app = App::new();
    app.add_plugins(VmuxPlugin::new());
    assert!(
        app.is_plugin_added::<TmuxPlugin>(),
        "tmux WM chord domain should be registered for composition/tests"
    );
}

#[cfg(test)]
#[test]
fn vmux_plugin_registers_runtime_plugin() {
    let mut app = App::new();
    app.add_plugins(VmuxPlugin::new());
    assert!(
        app.is_plugin_added::<RuntimePlugin>(),
        "winit runner + RuntimeState must come from VmuxPlugin, not WindowsPlugin"
    );
}
