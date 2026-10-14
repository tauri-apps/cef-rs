//! Bevy [`Event`] types for the windowing layer (winit → ECS), mirroring `bevy_window::event`.

use bevy_ecs::prelude::Event;
use winit::event::WindowEvent;
use winit::window::WindowId;

/// One winit [`WindowEvent`] for an OSR shell window, drained on Bevy `Update` after
/// [`crate::window::system::ingest_winit_window_dispatches_system`].
#[derive(Event, Clone, Debug)]
pub struct OsrHostWindowDispatch {
    pub window_id: WindowId,
    pub event: WindowEvent,
}
