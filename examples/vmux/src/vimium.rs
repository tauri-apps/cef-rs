//! Vimium-style keyboard UX for vmux (Bevy plugin + state machine), listed at crate root alongside
//! other feature domains (`settings`, `pane`, `window`, `browser`).
//!
//! Set **`VMUX_VIMIUM_INPUT_LOG`** to trace keyboard / IME vs CEF (see [`crate::log`] for log files).
//!
//! **Public surface:** only [`VimiumPlugin`] (re-exported from the crate root). Implementation is
//! private to this module; [`crate::window`] / [`crate::tmux`] use [`crate::input::ShellKeyboardStateResource`]
//! and [`crate::input::shell::ShellKeyboardHandler`] only.

mod editable_gating;
mod modes;
mod scroll;
mod state;
mod system;
mod window_input;

pub(crate) use state::VimiumStateResource;
pub(crate) use system::apply_shortcut_key_replay_events_system;

use bevy_app::{App as BevyApp, Plugin, Update};
use bevy_ecs::schedule::IntoSystemConfigs;

use crate::browser::cef::queue::apply_browser_ui_ops_system;

/// Registers vimium [`Update`](bevy_app::Update) systems (shortcut replay, link-hint feed, find queue, snapshot sync).
pub struct VimiumPlugin;

impl Plugin for VimiumPlugin {
    fn build(&self, app: &mut BevyApp) {
        // Deferred keys: DOM probe enqueues `SetEditableFocusHint` before [`ShortcutKeyReplayEvent`].
        // Run after `apply_browser_ui_ops_system` so replay sees updated hints; replay also passes
        // ECS `(Entity, browser_id)` so `browser_id_for_window` still resolves if `windows_store`
        // lags the first frame.
        app.init_resource::<state::VimiumStateResource>()
            .init_resource::<state::VimiumRuntimeResource>()
            .add_systems(
                Update,
                (
                    system::apply_shortcut_key_replay_events_system,
                    system::apply_link_hint_feed_events_system,
                    system::apply_find_mode_key_queue_system,
                    system::sync_vimium_runtime_resource_system,
                )
                    .after(apply_browser_ui_ops_system),
            );
    }
}
