//! Shell keyboard / hint transport into Bevy (bevy_input-style plugin + system set).
//!
//! OSR keyboard forward (winit → CEF after shell gating) lives in [`keyboard_forward`] and is drained
//! from the winit dispatch path / tmux WM handler for per-event snapshot alignment.

pub(crate) mod keyboard_forward;
pub(crate) mod shell;
pub(crate) mod system;

/// Shell-side keyboard handler state ([`crate::vimium::state::VimiumState`]). Window and tmux use this
/// type (and [`shell::ShellKeyboardHandler`]) only from `input`, not from `vimium`.
pub(crate) type ShellKeyboardStateResource = crate::vimium::VimiumStateResource;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::{IntoSystemConfigs, IntoSystemSetConfigs};

use crate::browser::cef::focus::dispatch_editable_focus_probe_requests_system;
use crate::browser::cef::queue::apply_browser_ui_ops_system;

/// Registers shell→ECS input events, [`EditableFocusQueues`](system::EditableFocusQueues), and
/// queue-drain systems in [`InputSystems`](system::InputSystems). See [`keyboard_forward`] for
/// CEF key forwarding helpers (invoked from `window` / `tmux`, not as standalone ECS systems).
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<crate::browser::event::ShortcutKeyReplayEvent>()
            .add_event::<crate::browser::event::LinkHintFeedEvent>()
            .init_resource::<system::EditableFocusQueues>()
            .configure_sets(
                Update,
                system::InputSystems.after(apply_browser_ui_ops_system),
            )
            .add_systems(
                Update,
                system::process_editable_focus_probe_queue_system.in_set(system::InputSystems),
            )
            .add_systems(
                Update,
                system::process_editable_shortcut_key_replay_queue_system
                    .after(dispatch_editable_focus_probe_requests_system),
            );
    }
}
