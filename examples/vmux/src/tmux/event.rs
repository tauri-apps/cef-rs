//! Bevy [`Event`] types for the tmux WM domain.

use bevy_ecs::prelude::{Entity, Event};
use winit::event::KeyEvent;
use winit::window::WindowId;

/// Manual / tmux WM mode: a shell `KeyboardInput` to route after [`crate::window::dispatch::window_dispatch_prelude`].
///
/// Consumed by [`crate::tmux::system::process_tmux_wm_shell_keyboard_events_system`] (chained after
/// [`crate::window::system::apply_osr_host_window_dispatches_system`]).
#[derive(Event, Clone, Debug)]
pub struct TmuxWmShellKeyboardEvent {
    pub window_id: WindowId,
    pub key_event: KeyEvent,
    pub ecs_browser: Option<(Entity, i32)>,
}
