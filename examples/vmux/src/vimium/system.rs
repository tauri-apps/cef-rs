//! Bevy systems for vimium shell input (shortcut replay, link-hint feed, find queue).

use std::collections::HashMap;

use bevy_ecs::entity::Entity;
use bevy_ecs::event::{EventReader, EventWriter};
use bevy_ecs::prelude::{NonSendMut, Query, Res, ResMut};
use winit::window::WindowId;

use crate::browser::cef::entity::{BrowserId, BrowserWindowId};
use crate::browser::cef::facet::EditableFocusHint;
use crate::browser::event::{
    BrowserEventBatch, LinkHintsHideBrowserEvent, LinkHintsShowBrowserEvent, NavigateBrowserEvent,
    ReloadBrowserEvent, ShortcutKeyReplayEvent,
};
use crate::runtime::RuntimeState;

use super::state::{VimiumRuntimeResource, VimiumStateResource};
use super::window_input::{
    apply_link_hint_feed_command, drain_find_mode_keys, try_handle_vimium_keys_after_editable_probe,
};
use crate::browser::cef::facet::EditableFocusSnapshot;
use crate::input::shell as shell_input;

pub(crate) fn apply_shortcut_key_replay_events_system(
    mut events: EventReader<ShortcutKeyReplayEvent>,
    mut osr_host: NonSendMut<crate::browser::cef::shell::OsrHostState>,
    mut rt: ResMut<RuntimeState>,
    mut vim: ResMut<VimiumStateResource>,
    browser_q: Query<(Entity, &BrowserWindowId, &BrowserId, &EditableFocusHint)>,
    mut navigate_events: EventWriter<NavigateBrowserEvent>,
    mut reload_events: EventWriter<ReloadBrowserEvent>,
    mut link_hints_show_events: EventWriter<LinkHintsShowBrowserEvent>,
    mut link_hints_hide_events: EventWriter<LinkHintsHideBrowserEvent>,
) {
    let mut editable_focus: EditableFocusSnapshot = HashMap::new();
    let mut ecs_by_window: HashMap<WindowId, (Entity, i32)> = HashMap::new();
    for (entity, wid, bid, hint) in browser_q.iter() {
        editable_focus.insert(bid.0, hint.0);
        ecs_by_window.insert(wid.0, (entity, bid.0));
    }
    for event in events.read() {
        let ecs = ecs_by_window.get(&event.window_id).copied();
        let mut out = BrowserEventBatch::default();
        let handled = try_handle_vimium_keys_after_editable_probe(
            &mut osr_host,
            &mut rt,
            &mut vim.0,
            event.window_id,
            &event.event,
            ecs,
            &editable_focus,
            &mut out,
        );
        if shell_input::shell_input_trace_enabled() {
            let ev = &event.event;
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                window_id = ?event.window_id,
                vimium_handled = handled,
                state = ?ev.state,
                repeat = ev.repeat,
                physical = ?ev.physical_key,
                logical = ?ev.logical_key,
                text = ?ev.text,
                "shell_input: KeyboardInput (after try_handle_vimium_keys, shortcut replay system)",
            );
        }
        for ev in out.navigate {
            navigate_events.send(ev);
        }
        for ev in out.reload {
            reload_events.send(ev);
        }
        for ev in out.link_hints_show {
            link_hints_show_events.send(ev);
        }
        for ev in out.link_hints_hide {
            link_hints_hide_events.send(ev);
        }
    }
}

pub(crate) fn apply_link_hint_feed_events_system(
    mut events: EventReader<crate::browser::event::LinkHintFeedEvent>,
    mut osr_host: NonSendMut<crate::browser::cef::shell::OsrHostState>,
    mut vim: ResMut<VimiumStateResource>,
    mut link_hints_hide_events: EventWriter<LinkHintsHideBrowserEvent>,
) {
    for event in events.read() {
        let mut out = BrowserEventBatch::default();
        apply_link_hint_feed_command(
            &mut osr_host,
            &mut vim.0,
            event.window_id,
            event.browser_id,
            event.ch,
            event.still_active,
            event.hint_label_width,
            &mut out,
        );
        for ev in out.link_hints_hide {
            link_hints_hide_events.send(ev);
        }
    }
}

pub(crate) fn apply_find_mode_key_queue_system(
    mut osr_host: NonSendMut<crate::browser::cef::shell::OsrHostState>,
    mut rt: ResMut<RuntimeState>,
    mut vim: ResMut<VimiumStateResource>,
) {
    drain_find_mode_keys(&mut osr_host, &mut rt, &mut vim.0);
}

pub(crate) fn sync_vimium_runtime_resource_system(
    vim: Res<VimiumStateResource>,
    mut vimium_runtime: ResMut<VimiumRuntimeResource>,
) {
    vimium_runtime.snapshot = vim.0.snapshot();
}
