//! Shell → Bevy queues for editable-focus probing and shortcut replay (see [`super::InputPlugin`]).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bevy_ecs::event::EventWriter;
use bevy_ecs::prelude::{Res, Resource};
use bevy_ecs::schedule::SystemSet;
use winit::event::KeyEvent;
use winit::window::WindowId;

use crate::browser::cef::focus::{EditableFocusProbeRequest, EditableShortcutKeyReplayRequest};

/// Label for systems that drain shell queues into Bevy events (mirrors bevy_input’s `InputSystems`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct InputSystems;

/// Queues editable-focus work from the winit shell; [`process_editable_focus_probe_queue_system`] /
/// [`process_editable_shortcut_key_replay_queue_system`] turn these into [`EditableFocusProbeRequest`] /
/// [`EditableShortcutKeyReplayRequest`] for the dispatch systems in [`crate::browser::cef::focus`].
#[derive(Resource, Clone)]
pub struct EditableFocusQueues {
    pub pending_probes: Arc<Mutex<VecDeque<i32>>>,
    pub pending_shortcut_key_replays: Arc<Mutex<VecDeque<(i32, WindowId, KeyEvent)>>>,
}

impl Default for EditableFocusQueues {
    fn default() -> Self {
        Self {
            pending_probes: Arc::new(Mutex::new(VecDeque::new())),
            pending_shortcut_key_replays: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

pub fn process_editable_focus_probe_queue_system(
    queues: Res<EditableFocusQueues>,
    mut writer: EventWriter<EditableFocusProbeRequest>,
) {
    let pending: Vec<i32> = queues
        .pending_probes
        .lock()
        .ok()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    for browser_id in pending {
        writer.send(EditableFocusProbeRequest { browser_id });
    }
}

pub fn process_editable_shortcut_key_replay_queue_system(
    queues: Res<EditableFocusQueues>,
    mut writer: EventWriter<EditableShortcutKeyReplayRequest>,
) {
    let pending: Vec<(i32, WindowId, KeyEvent)> = queues
        .pending_shortcut_key_replays
        .lock()
        .ok()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    for (browser_id, window_id, event) in pending {
        writer.send(EditableShortcutKeyReplayRequest {
            browser_id,
            window_id,
            event,
        });
    }
}
