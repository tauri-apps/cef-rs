//! ECS systems for tmux WM routing (shell keyboard → chords / Vimium / CEF).

use std::collections::{HashMap, VecDeque};

use bevy_ecs::entity::Entity;
use bevy_ecs::event::EventReader;
use bevy_ecs::event::EventWriter;
use bevy_ecs::prelude::{Local, NonSendMut, Query, ResMut};
use winit::window::WindowId;

use crate::browser::cef::entity::{BrowserId, BrowserWindowId};
use crate::browser::cef::facet::{EditableFocusHint, EditableFocusSnapshot};
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::{BrowserEventBatch, BrowserShellEffectBatchEvent};
use crate::input::shell::ShellKeyboardHandler;
use crate::input::ShellKeyboardStateResource;
use crate::runtime::RuntimeState;
use crate::window::dispatch::try_enqueue_shell_keyboard_forward_to_cef;
use crate::input::keyboard_forward::{PendingOsrKeyboardForward, drain_pending_osr_keyboard_forwards};

use super::event::TmuxWmShellKeyboardEvent;
use super::keyboard::try_handle_wm_keyboard_event;

/// Try tmux WM chords, then Vimium, then CEF keyboard forward; fan out browser events once per frame.
pub fn process_tmux_wm_shell_keyboard_events_system(
    mut osr_host: NonSendMut<OsrHostState>,
    mut rt: ResMut<RuntimeState>,
    mut shell_keyboard: ResMut<ShellKeyboardStateResource>,
    browser_q: Query<(Entity, &BrowserWindowId, &BrowserId)>,
    focus_q: Query<(&BrowserId, &EditableFocusHint)>,
    mut tmux_events: EventReader<TmuxWmShellKeyboardEvent>,
    mut shell_effect_batches: EventWriter<BrowserShellEffectBatchEvent>,
    mut keyboard_forward_pending: Local<VecDeque<PendingOsrKeyboardForward>>,
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
    let handler: &mut dyn ShellKeyboardHandler = &mut shell_keyboard.0;

    let mut saw_tmux = false;
    for ev in tmux_events.read() {
        saw_tmux = true;
        let ecs = ev
            .ecs_browser
            .or_else(|| by_window.get(&ev.window_id).copied());
        if try_handle_wm_keyboard_event(
            &mut osr_host,
            &mut rt,
            &ev.key_event,
            ev.window_id,
            ecs,
            &editable_focus,
        ) {
            drain_pending_osr_keyboard_forwards(
                &mut osr_host,
                &mut rt,
                &mut keyboard_forward_pending,
                &mut out,
            );
            continue;
        }
        if handler.try_handle_vimium_keys(
            &mut osr_host,
            &mut rt,
            ev.window_id,
            &ev.key_event,
            ecs,
            &editable_focus,
            &mut out,
        ) {
            drain_pending_osr_keyboard_forwards(
                &mut osr_host,
                &mut rt,
                &mut keyboard_forward_pending,
                &mut out,
            );
            continue;
        }
        try_enqueue_shell_keyboard_forward_to_cef(
            &mut osr_host,
            ev.window_id,
            ev.key_event.clone(),
            &rt,
            handler,
            &mut keyboard_forward_pending,
        );
        drain_pending_osr_keyboard_forwards(
            &mut osr_host,
            &mut rt,
            &mut keyboard_forward_pending,
            &mut out,
        );
    }

    if saw_tmux {
        shell_effect_batches.send(BrowserShellEffectBatchEvent(out));
    }
}
