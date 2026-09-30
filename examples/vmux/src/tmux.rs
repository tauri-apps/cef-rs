//! Tmux plugin: manual / BSP tiling ([`pane_tree`], [`layout`]) and tmux-style WM chords ([`keyboard`], [`system`]).
//!
//! Manual-mode shell keys are emitted as [`event::TmuxWmShellKeyboardEvent`] from the window dispatch
//! path and handled in [`system::process_tmux_wm_shell_keyboard_events_system`] ([`TmuxPlugin`], after
//! [`crate::window::system::apply_osr_host_window_dispatches_system`]). Fanout runs next in [`BrowserPlugin`].
//! Generic title HUD formatting lives in [`crate::window`](crate::window) (wm helpers).

pub(crate) mod event;
pub(crate) mod keyboard;
pub(crate) mod layout;
pub(crate) mod pane_tree;
pub(crate) mod system;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::Resource;
use bevy_ecs::schedule::IntoSystemConfigs;

/// Composition marker and future hook for tmux-only ECS state.
#[derive(Resource, Default)]
struct TmuxPluginMarker;

/// Registers the tmux WM domain (BSP tiling + chord handling on the winit path).
pub struct TmuxPlugin;

impl Plugin for TmuxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TmuxPluginMarker>()
            .add_event::<event::TmuxWmShellKeyboardEvent>()
            .add_systems(
                Update,
                system::process_tmux_wm_shell_keyboard_events_system
                    .after(crate::window::system::apply_osr_host_window_dispatches_system),
            );
    }
}
