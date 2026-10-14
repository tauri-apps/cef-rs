//! ECS systems for the winit-backed runner ([`crate::runtime::RuntimePlugin`] lives in `runtime.rs`).

use std::sync::atomic::Ordering;

use bevy_app::AppExit;
use bevy_ecs::event::{EventReader, EventWriter};
use bevy_ecs::prelude::{Res, ResMut, Resource};
use bevy_ecs::world::World;

use super::event::{RuntimeState, ShutdownFlag};
use super::ffi_callbacks::request_quit;
use crate::browser::cef::entity::CefBrowserHandles;
use crate::browser::cef::lifecycle::RequestCloseAllBrowsersEvent;

/// Runs `f` with [`OsrHostState`] (`NonSend`) and [`RuntimeState`]. Nested [`World::resource_scope`]
/// avoids overlapping `&mut World` borrows with the OSR host `NonSend` resource.
pub fn with_osr_host_runtime<F, R>(world: &mut World, f: F) -> R
where
    F: FnOnce(&mut crate::browser::cef::shell::OsrHostState, &mut RuntimeState) -> R,
{
    world.resource_scope(|world, mut rt| {
        let mut osr_host =
            world.non_send_resource_mut::<crate::browser::cef::shell::OsrHostState>();
        f(&mut osr_host, &mut *rt)
    })
}

#[derive(Resource, Debug, Default)]
pub struct AppExitRequested(pub bool);

/// Unix signals must not flip [`ShutdownFlag`] directly: that flag gates `run_winit` exit and must
/// only become true after browsers close. Drain into the same path as AppKit Quit.
pub fn drain_signal_quit_to_request_quit_system(signal: Option<Res<super::event::SignalQuitFlag>>) {
    let Some(signal) = signal else {
        return;
    };
    if signal.0.swap(false, Ordering::AcqRel) {
        request_quit();
    }
}

pub fn emit_app_exit_on_shutdown_signal(
    shutdown: Res<ShutdownFlag>,
    mut requested: ResMut<AppExitRequested>,
    mut app_exit: EventWriter<AppExit>,
) {
    if shutdown.0.load(Ordering::Acquire) && !requested.0 {
        requested.0 = true;
        app_exit.send(AppExit::Success);
    }
}

pub fn handle_app_exit_for_graceful_shutdown(
    shutdown: Res<ShutdownFlag>,
    handles: Res<CefBrowserHandles>,
    mut app_exit: EventReader<AppExit>,
    mut requested: ResMut<AppExitRequested>,
    mut close_all: EventWriter<RequestCloseAllBrowsersEvent>,
) {
    if app_exit.read().next().is_none() || requested.0 {
        return;
    }
    requested.0 = true;
    if handles.0.lock().map(|g| !g.is_empty()).unwrap_or(false) {
        // Match shell Cmd+Q (`QuitCloseAllBrowsersBrowserEvent`) and menu Quit: force-close so
        // windowless OSR teardown cannot stall on `close_browser(false)`.
        close_all.send(RequestCloseAllBrowsersEvent { force_close: true });
    } else {
        shutdown.0.store(true, Ordering::Release);
    }
}
