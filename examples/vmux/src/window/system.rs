//! ECS systems and resources for winit → OSR window dispatch (Bevy `Update`), mirroring `bevy_window::system`.
//!
//! The custom winit runner appends pairs into [`PendingWindowEvents`] during its pump (see `runtime/runner`);
//! [`ingest_winit_window_dispatches_system`] turns each queued pair into an [`OsrHostWindowDispatch`](crate::window::event::OsrHostWindowDispatch);
//! [`apply_osr_host_window_dispatches_system`] runs after browser entity spawn/despawn (see [`crate::browser::BrowserPlugin`]) and before [`crate::browser::cef::queue::apply_browser_ui_ops_system`], emitting
//! [`TmuxWmShellKeyboardEvent`](crate::tmux::event::TmuxWmShellKeyboardEvent) or calling
//! [`crate::window::dispatch::handle_window_event_inner`] per dispatch, then
//! [`crate::input::keyboard_forward::drain_pending_osr_keyboard_forwards`]. [`TmuxPlugin`] runs
//! [`crate::tmux::system::process_tmux_wm_shell_keyboard_events_system`] after apply; [`BrowserPlugin`] runs
//! [`crate::browser::system::fanout_browser_shell_effect_batch_events_system`] after that; this module chains
//! ingest → apply and schedules [`apply_wm_hud_to_shells_after_dispatches_system`] after fanout.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use bevy_ecs::entity::Entity;
use bevy_ecs::event::EventReader;
use bevy_ecs::event::EventWriter;
use bevy_ecs::prelude::{NonSendMut, Query, Res, ResMut, Resource};
use winit::event::WindowEvent;
use winit::window::WindowId;

use crate::browser::cef::entity::{BrowserId, BrowserWindowId};
use crate::browser::cef::facet::{EditableFocusHint, EditableFocusSnapshot};
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::{BrowserEventBatch, BrowserShellEffectBatchEvent};
use crate::input::shell::ShellKeyboardHandler;
use crate::input::ShellKeyboardStateResource;
use crate::runtime::LinkHintsNavPending;
use crate::runtime::RuntimeState;
use crate::input::keyboard_forward::drain_pending_osr_keyboard_forwards;
use crate::tmux::event::TmuxWmShellKeyboardEvent;

use super::dispatch;
use super::event::OsrHostWindowDispatch;
use super::wm;

pub fn apply_wm_hud_to_shells_after_dispatches_system(
    mut osr_host: NonSendMut<OsrHostState>,
    mut rt: ResMut<RuntimeState>,
) {
    wm::apply_wm_hud_to_all_shell_windows(&mut osr_host, &mut rt);
}

/// Winit can deliver `KeyboardInput` before `ModifiersChanged` for the same physical press; vmux
/// stores modifiers in [`RuntimeState::mods_winit`] only on `ModifiersChanged`, so reorder pairs
/// `(KeyboardInput, ModifiersChanged)` for the same window to apply modifiers first.
fn reorder_modifiers_before_following_keyboard(events: &mut Vec<(WindowId, WindowEvent)>) {
    for _ in 0..events.len().saturating_add(2) {
        let mut any = false;
        for i in 0..events.len().saturating_sub(1) {
            let swap = events[i].0 == events[i + 1].0
                && matches!(events[i].1, WindowEvent::KeyboardInput { .. })
                && matches!(events[i + 1].1, WindowEvent::ModifiersChanged(_));
            if swap {
                events.swap(i, i + 1);
                any = true;
            }
        }
        if !any {
            break;
        }
    }
}

#[derive(Resource, Clone)]
pub struct PendingWindowEvents(pub Arc<Mutex<VecDeque<(WindowId, WindowEvent)>>>);

impl Default for PendingWindowEvents {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(VecDeque::new())))
    }
}

/// Drain the winit queue into Bevy [`OsrHostWindowDispatch`] events (preserves order).
pub fn ingest_winit_window_dispatches_system(
    pending: Res<PendingWindowEvents>,
    mut writer: EventWriter<OsrHostWindowDispatch>,
) {
    let mut batch: Vec<(WindowId, WindowEvent)> = pending
        .0
        .lock()
        .ok()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    reorder_modifiers_before_following_keyboard(&mut batch);
    for (window_id, event) in batch {
        writer.send(OsrHostWindowDispatch { window_id, event });
    }
}

/// Apply each winit dispatch in order. Manual WM keys emit [`TmuxWmShellKeyboardEvent`] for
/// [`crate::tmux::system::process_tmux_wm_shell_keyboard_events_system`]; other routing uses
/// [`dispatch::handle_window_event_inner`](dispatch::handle_window_event_inner).
pub fn apply_osr_host_window_dispatches_system(
    mut osr_host: NonSendMut<OsrHostState>,
    mut rt: ResMut<RuntimeState>,
    mut shell_keyboard: ResMut<ShellKeyboardStateResource>,
    mut link_hints_nav_pending: ResMut<LinkHintsNavPending>,
    browser_q: Query<(Entity, &BrowserWindowId, &BrowserId)>,
    focus_q: Query<(&BrowserId, &EditableFocusHint)>,
    mut dispatches: EventReader<OsrHostWindowDispatch>,
    mut tmux_wm_keyboard_events: EventWriter<TmuxWmShellKeyboardEvent>,
    mut shell_effect_batches: EventWriter<BrowserShellEffectBatchEvent>,
) {
    let mut by_window: HashMap<WindowId, (Entity, i32)> = HashMap::new();
    for (entity, wid, bid) in browser_q.iter() {
        by_window.insert(wid.0, (entity, bid.0));
    }
    let mut editable_focus: EditableFocusSnapshot = HashMap::new();
    for (bid, hint) in focus_q.iter() {
        editable_focus.insert(bid.0, hint.0);
    }
    let mut out = BrowserEventBatch::default();
    let mut keyboard_forward_pending = VecDeque::new();
    let mut saw_dispatch = false;
    for dispatch in dispatches.read() {
        saw_dispatch = true;
        let ecs = by_window.get(&dispatch.window_id).copied();
        let handler: &mut dyn ShellKeyboardHandler = &mut shell_keyboard.0;
        dispatch::window_dispatch_prelude(
            &mut osr_host,
            handler,
            &mut link_hints_nav_pending,
            &mut out,
        );
        if let winit::event::WindowEvent::KeyboardInput { event, .. } = &dispatch.event {
            if crate::tmux::keyboard::should_handle_tmux_wm_keys(&*osr_host.key_settings) {
                tmux_wm_keyboard_events.send(TmuxWmShellKeyboardEvent {
                    window_id: dispatch.window_id,
                    key_event: event.clone(),
                    ecs_browser: ecs,
                });
                drain_pending_osr_keyboard_forwards(
                    &mut osr_host,
                    &mut rt,
                    &mut keyboard_forward_pending,
                    &mut out,
                );
                continue;
            }
        }
        dispatch::handle_window_event_inner(
            &mut osr_host,
            &mut rt,
            handler,
            dispatch.window_id,
            dispatch.event.clone(),
            ecs,
            &editable_focus,
            &mut out,
            &mut keyboard_forward_pending,
        );
        drain_pending_osr_keyboard_forwards(
            &mut osr_host,
            &mut rt,
            &mut keyboard_forward_pending,
            &mut out,
        );
    }
    if saw_dispatch {
        shell_effect_batches.send(BrowserShellEffectBatchEvent(out));
    }
}

/// Initial [`RuntimeState::wm_geometry_layout`](crate::runtime::RuntimeState::wm_geometry_layout) from settings.
pub fn sync_window_manager_runtime_from_settings(
    mut rt: ResMut<crate::runtime::RuntimeState>,
    settings: Res<crate::settings::SettingsResource>,
) {
    rt.wm_geometry_layout = settings.0.wm_initial_geometry_layout;
}
