//! Windowing, tiling / WM, and ECS window registry for vmux.
//!
//! Layout mirrors [bevy_window](https://github.com/bevyengine/bevy/tree/main/crates/bevy_window/src), but child modules
//! stay mostly crate-private: [`WindowsPlugin`] wires them here. [`event`], [`dispatch`], [`registry`],
//! [`system`], [`tiling`], and [`wm`] are `pub(crate)` for cross-module paths; leaf modules like [`focus`] stay private.
//! Import via `crate::window::<submodule>::…` (see workspace rule `vmux-bevy-module-entry.mdc`).

pub(crate) mod dispatch;
pub(crate) mod event;
mod focus;
mod layout;
mod pane_id;
pub(crate) mod registry;
mod session;
pub(crate) mod system;
pub(crate) mod tiling;
pub(crate) mod wm;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::schedule::IntoSystemConfigs;

use crate::runtime::{LinkHintsNavPending, ingest_link_hints_nav_invalidate_events_system};
use crate::settings::VmuxSettingsStartup;

/// Window/session domain: winit→Bevy dispatch, registry, tiling state ([`crate::runtime::RuntimePlugin`] is added by [`crate::VmuxPlugin`]).
pub struct WindowsPlugin;

impl Plugin for WindowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<event::OsrHostWindowDispatch>()
            .init_resource::<system::PendingWindowEvents>()
            .init_resource::<LinkHintsNavPending>()
            .add_systems(
                Startup,
                system::sync_window_manager_runtime_from_settings.after(VmuxSettingsStartup),
            )
            .init_resource::<registry::WindowRegistryState>()
            .add_systems(
                Update,
                (
                    ingest_link_hints_nav_invalidate_events_system,
                    (
                        system::ingest_winit_window_dispatches_system,
                        system::apply_osr_host_window_dispatches_system,
                    )
                        .chain(),
                    system::apply_wm_hud_to_shells_after_dispatches_system
                        .after(crate::browser::system::fanout_browser_shell_effect_batch_events_system),
                    registry::sync_window_components_with_cef,
                ),
            )
            .init_resource::<session::SessionState>()
            .init_resource::<focus::FocusedPane>()
            .init_resource::<layout::DefaultSplitAxis>();
    }
}
