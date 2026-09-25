//! Bevy custom runner + winit-backed main-thread event loop for vmux (`crate::runtime`).
//!
//! The winit [`UserEvent`](event::UserEvent) payload is split into [`AppEvent`](event::AppEvent),
//! [`CefEvent`](event::CefEvent), and [`ShellInputEvent`](event::ShellInputEvent) (shell input
//! transport: shortcut replay, link hints; keybinding plugins consume via Bevy events).
//! [`AppUserEvent`] is a type alias for [`UserEvent`](event::UserEvent) (back-compat).

use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::IntoSystemConfigs;

pub use crate::browser::cef::entity::{
    BrowserDespawnEvent, BrowserEntities, BrowserId, BrowserSpawnEvent, BrowserWindowId,
    CefBrowserHandle, CefBrowserHandles, CefBrowserHandlesInner, OsrPaintBindGroup,
    OsrViewLogicalSize,
};
pub use crate::browser::cef::lifecycle::{
    BrowserCloseGuardsInner, BrowserCloseGuardsResource, BrowserLifecycleInner,
    BrowserLifecycleResource, RequestCloseAllBrowsersEvent,
    apply_close_all_browsers_requests_system,
};

mod event;
mod ffi_callbacks;
mod runner;
mod systems;

pub use crate::browser::cef::active::apply_pending_set_active_browser_system;
pub use crate::input::system::EditableFocusQueues;
pub use ffi_callbacks::{
    ffi_device_scale_factor, ffi_gpu, ffi_osr_index, register_gpu_only_for_ffi,
    register_gpu_runtime_for_ffi, register_osr_index_and_scale_for_ffi,
    register_winit_proxy_for_ffi, request_quit, schedule_cef_work, send_user_event, try_ffi_gpu,
    try_ffi_osr_index,
};
pub use systems::{
    AppExitRequested, drain_signal_quit_to_request_quit_system,
    emit_app_exit_on_shutdown_signal, handle_app_exit_for_graceful_shutdown,
};
// Used as `crate::runtime::…` from other modules; not referenced inside this file.
pub use crate::browser::event::{LinkHintFeedEvent, ShortcutKeyReplayEvent};
pub use event::{
    AppEvent, AppUserEvent, CefEvent, CefPumpDeadline, LinkHintsNavPending, RuntimeState,
    ShellInputEvent, ShutdownFlag, SignalQuitFlag, UserEvent, WmAction,
    ingest_link_hints_nav_invalidate_events_system,
};
pub use runner::{VmuxMainEventLoop, WinitAppRunnerState, build_event_loop, run_winit};

/// Registers [`RuntimeState`](event::RuntimeState), shutdown-driven [`AppExit`](bevy_app::AppExit), and the custom winit [`runner`] entry.
pub struct RuntimePlugin;

impl RuntimePlugin {
    pub const fn new() -> Self {
        Self
    }
}

impl Plugin for RuntimePlugin {
    fn build(&self, app: &mut App) {
        use systems::{
            drain_signal_quit_to_request_quit_system, emit_app_exit_on_shutdown_signal,
            handle_app_exit_for_graceful_shutdown,
        };

        app.init_resource::<event::RuntimeState>()
            .init_resource::<systems::AppExitRequested>()
            .add_systems(
                Update,
                (
                    drain_signal_quit_to_request_quit_system,
                    emit_app_exit_on_shutdown_signal,
                    handle_app_exit_for_graceful_shutdown,
                )
                    .chain(),
            );

        app.set_runner(|mut bevy_app| {
            let Some(mut holder) = bevy_app
                .world_mut()
                .remove_non_send_resource::<runner::VmuxMainEventLoop>()
            else {
                panic!(
                    "vmux: insert_non_send_resource(VmuxMainEventLoop(event_loop)) before VmuxPlugin (browser main)"
                );
            };
            runner::run_winit(&mut holder.0, bevy_app)
        });
    }
}
