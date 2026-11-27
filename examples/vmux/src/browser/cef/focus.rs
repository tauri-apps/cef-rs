//! Editable-focus DOM probing for shortcut routing.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bevy_ecs::event::{Event, EventReader};

use cef::ImplDomdocument as _;
use cef::ImplFrame as _;
use cef::*;
use winit::event::KeyEvent;
use winit::window::WindowId;

#[derive(Event, Clone, Debug)]
pub struct EditableFocusProbeRequest {
    pub browser_id: i32,
}

#[derive(Event, Clone, Debug)]
pub struct EditableShortcutKeyReplayRequest {
    pub browser_id: i32,
    pub window_id: WindowId,
    pub event: KeyEvent,
}

pub fn enqueue_probe_request(pending: &Arc<Mutex<VecDeque<i32>>>, browser_id: i32) {
    if let Ok(mut q) = pending.lock() {
        q.push_back(browser_id);
    }
}

pub fn enqueue_shortcut_key_replay_request(
    pending: &Arc<Mutex<VecDeque<(i32, WindowId, KeyEvent)>>>,
    browser_id: i32,
    window_id: WindowId,
    event: KeyEvent,
) {
    if let Ok(mut q) = pending.lock() {
        q.push_back((browser_id, window_id, event));
    }
}

fn dom_element_is_text_entry_host(node: &Domnode) -> bool {
    use ImplDomnode as _;
    if node.is_element() == 0 {
        return false;
    }
    let tag = CefStringUtf8::from(&CefStringUtf16::from(&node.element_tag_name())).to_string();
    let tag = tag.to_lowercase();
    match tag.as_str() {
        "textarea" => true,
        "select" => true,
        "input" => {
            let ty = CefString::from("type");
            let raw = node.element_attribute(Some(&ty));
            let t = CefStringUtf8::from(&CefStringUtf16::from(&raw))
                .to_string()
                .to_lowercase();
            !matches!(
                t.trim(),
                "hidden"
                    | "button"
                    | "submit"
                    | "reset"
                    | "checkbox"
                    | "radio"
                    | "file"
                    | "image"
                    | "range"
                    | "color"
            )
        }
        _ => {
            let role = CefString::from("role");
            if node.has_element_attribute(Some(&role)) == 0 {
                return false;
            }
            let raw = node.element_attribute(Some(&role));
            let r = CefStringUtf8::from(&CefStringUtf16::from(&raw))
                .to_string()
                .to_lowercase();
            matches!(
                r.trim(),
                "textbox" | "searchbox" | "combobox" | "spinbutton"
            )
        }
    }
}

fn dom_focused_context_allows_typing(mut node: Domnode, max_ancestors: usize) -> bool {
    use ImplDomnode as _;
    for _ in 0..max_ancestors {
        if node.is_editable() != 0 {
            return true;
        }
        if dom_element_is_text_entry_host(&node) {
            return true;
        }
        if node.is_element() != 0 {
            let ce = CefString::from("contenteditable");
            if node.has_element_attribute(Some(&ce)) != 0 {
                let raw = node.element_attribute(Some(&ce));
                let v = CefStringUtf8::from(&CefStringUtf16::from(&raw)).to_string();
                let v = v.trim().to_lowercase();
                if v.is_empty() || v == "true" || v == "plaintext-only" {
                    return true;
                }
                if v != "false" && v != "inherit" {
                    return true;
                }
            }
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }
    false
}

pub fn run_editable_focus_probe_on_ui(browser_id: i32) {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);
    let browser = crate::browser::cef::lookup::cef_browser_by_id(browser_id);
    let Some(browser) = browser else {
        return;
    };
    let Some(frame) = browser.focused_frame().or_else(|| browser.main_frame()) else {
        return;
    };
    let mut visitor = EditableFocusDomVisitor::new(browser_id);
    frame.visit_dom(Some(&mut visitor));
}

pub(crate) fn dispatch_editable_focus_probe_requests_system(
    mut requests: EventReader<EditableFocusProbeRequest>,
) {
    for ev in requests.read() {
        schedule_editable_focus_probe(ev.browser_id);
    }
}

pub(crate) fn dispatch_editable_shortcut_key_replay_requests_system(
    mut requests: EventReader<EditableShortcutKeyReplayRequest>,
) {
    for ev in requests.read() {
        post_editable_probe_for_shortcut_key_replay(ev.browser_id, ev.window_id, ev.event.clone());
    }
}

pub fn schedule_editable_focus_probe(browser_id: i32) {
    let thread_id = ThreadId::UI;
    if currently_on(thread_id) == 0 {
        let mut task = ProbeEditableFocus::new(browser_id);
        post_task(thread_id, Some(&mut task));
        return;
    }
    run_editable_focus_probe_on_ui(browser_id);
}

pub fn post_editable_probe_for_shortcut_key_replay(
    browser_id: i32,
    window_id: WindowId,
    event: KeyEvent,
) {
    if currently_on(ThreadId::UI) != 0 {
        run_editable_focus_probe_on_ui(browser_id);
        crate::runtime::send_user_event(crate::runtime::UserEvent::Input(
            crate::runtime::ShellInputEvent::ShortcutKeyReplay(
                crate::browser::event::ShortcutKeyReplayEvent { window_id, event },
            ),
        ));
        return;
    }
    let mut task = EditableProbeWakeTask::new(browser_id, window_id, event);
    if post_task(ThreadId::UI, Some(&mut task)) == 0 {
        bevy_log::warn!(
            target: "vmux",
            pid = std::process::id(),
            "post_editable_probe_for_shortcut_key_replay: post_task failed"
        );
    }
}

wrap_domvisitor! {
    struct EditableFocusDomVisitor {
        browser_id: i32,
    }

    impl Domvisitor {
        fn visit(&self, document: Option<&mut Domdocument>) {
            let max_ancestors = crate::settings::DEFAULT_DOM_FOCUS_ANCESTOR_WALK_MAX;
            let editable = match document {
                None => false,
                Some(doc) => doc
                    .focused_node()
                    .map(|n| dom_focused_context_allows_typing(n, max_ancestors))
                    .unwrap_or(false),
            };
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::Cef(
                    crate::runtime::CefEvent::SetEditableFocusHint {
                        browser_id: self.browser_id,
                        editable,
                    },
                ),
            );
        }
    }
}

wrap_task! {
    struct ProbeEditableFocus {
        browser_id: i32,
    }

    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            run_editable_focus_probe_on_ui(self.browser_id);
        }
    }
}

wrap_task! {
    struct EditableProbeWakeTask {
        browser_id: i32,
        window_id: WindowId,
        event: KeyEvent,
    }

    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            run_editable_focus_probe_on_ui(self.browser_id);
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::Input(
                    crate::runtime::ShellInputEvent::ShortcutKeyReplay(
                        crate::browser::event::ShortcutKeyReplayEvent {
                            window_id: self.window_id,
                            event: self.event.clone(),
                        },
                    ),
                ),
            );
        }
    }
}
