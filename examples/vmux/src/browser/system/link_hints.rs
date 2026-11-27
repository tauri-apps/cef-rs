//! Link-hints overlay: JS templates, DOM precount visitor, and apply systems.

use bevy_ecs::event::EventReader;
use bevy_ecs::prelude::Res;
use cef::rc::Rc;
use cef::{
    Browser, CefString, CefStringUtf16, CefStringUtf8, Domdocument, Domvisitor, Frame,
    ImplDomdocument, ImplDomnode, ImplDomvisitor, ImplFrame, ImplTask, Task,
    ThreadId,
    WrapDomvisitor, WrapTask, currently_on, post_task, wrap_domvisitor, wrap_task,
};
use std::sync::{Arc, Mutex};

use crate::browser::cef::ForeignOsrIndexResource;
use crate::browser::cef::frames::blur_active_element_all_frames;
use crate::browser::cef::hintsfeed::{
    link_hints_feed_key_on_ui, link_hints_frame_list, link_hints_read_session_with_browser,
};
use crate::browser::cef::lookup::cef_browser_by_id;
use crate::browser::cef::osr::ForeignOsrIndex;
use crate::browser::event::{
    LinkHintsFeedKeyDeferredBrowserEvent, LinkHintsHideBrowserEvent, LinkHintsShowBrowserEvent,
};

use super::ui::request_window_redraw_for_browser;

const VMUX_LINK_HINTS_JS_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/link_hints.js"
));
const VMUX_LINK_HINTS_PRECOUNT_JS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/link_hints_precount.js"
));

pub(crate) fn vmux_link_hints_js_with_labels(label_start: usize, label_total: usize) -> String {
    VMUX_LINK_HINTS_JS_TEMPLATE
        .replace("__VMUX_LABEL_START__", &label_start.to_string())
        .replace("__VMUX_LABEL_TOTAL__", &label_total.to_string())
}

wrap_domvisitor! {
    struct HintPrecountReadVisitor {
        out: Arc<Mutex<usize>>,
    }

    impl Domvisitor {
        fn visit(&self, document: Option<&mut Domdocument>) {
            let n = match document {
                None => 0usize,
                Some(doc) => doc
                    .document()
                    .map(|root| {
                        let attr = CefString::from("data-vmux-hint-precount");
                        if root.has_element_attribute(Some(&attr)) == 0 {
                            return 0usize;
                        }
                        let raw = root.element_attribute(Some(&attr));
                        let s = CefStringUtf8::from(&CefStringUtf16::from(&raw)).to_string();
                        s.trim().parse::<usize>().unwrap_or(0)
                    })
                    .unwrap_or(0),
            };
            if let Ok(mut g) = self.out.lock() {
                *g = n;
            }
        }
    }
}

fn link_hints_precount_execute_on_frame(frame: &Frame) {
    let code = CefString::from(VMUX_LINK_HINTS_PRECOUNT_JS);
    let url = CefString::from("vmux://link-hints-precount");
    frame.execute_java_script(Some(&code), Some(&url), 0);
}

fn link_hints_read_precount_on_frame(frame: &Frame) -> usize {
    let out = Arc::new(Mutex::new(0usize));
    let mut visitor = HintPrecountReadVisitor::new(Arc::clone(&out));
    frame.visit_dom(Some(&mut visitor));
    out.lock().ok().map(|g| *g).unwrap_or(0)
}

fn link_hints_show_execute(browser: &Browser, browser_id: i32, hub: &ForeignOsrIndex) {
    // Drop focus traps (e.g. Google consent language control) so link hints can arm; do **not**
    // call this from scroll — see module comment on [`blur_active_element_all_frames`].
    blur_active_element_all_frames(browser);
    crate::browser::cef::message_loop_pump(4);
    let mut frames = link_hints_frame_list(browser);
    let mut wait = 0u32;
    while frames.is_empty() && wait < 16 {
        crate::browser::cef::message_loop_pump(4);
        frames = link_hints_frame_list(browser);
        wait += 1;
    }
    if frames.is_empty() {
        bevy_log::warn!(
            target: "vmux",
            pid = std::process::id(),
            "link_hints_show_execute: no frames for browser_id={browser_id} after pump retries (page still loading?)"
        );
        return;
    }
    let mut counts: Vec<usize> = Vec::with_capacity(frames.len());
    for f in &frames {
        link_hints_precount_execute_on_frame(f);
        crate::browser::cef::message_loop_pump(4);
        counts.push(link_hints_read_precount_on_frame(f));
    }
    let mut total: usize = counts.iter().sum();
    // Cookie / consent UIs often live in a subframe that appears a few pumps after the main frame.
    if total == 0 {
        for _ in 0..6 {
            crate::browser::cef::message_loop_pump(12);
            frames = link_hints_frame_list(browser);
            if frames.is_empty() {
                continue;
            }
            counts.clear();
            counts.reserve(frames.len());
            for f in &frames {
                link_hints_precount_execute_on_frame(f);
                crate::browser::cef::message_loop_pump(6);
                counts.push(link_hints_read_precount_on_frame(f));
            }
            total = counts.iter().sum();
            if total > 0 {
                break;
            }
        }
    }
    let mut label_start = 0usize;
    let url = CefString::from("vmux://link-hints");
    for (i, f) in frames.iter().enumerate() {
        let c = counts[i];
        if c == 0 {
            continue;
        }
        let js = vmux_link_hints_js_with_labels(label_start, total);
        let code = CefString::from(js.as_str());
        f.execute_java_script(Some(&code), Some(&url), 0);
        label_start += c;
    }
    crate::browser::cef::message_loop_pump(8);
    let _snap = link_hints_read_session_with_browser(browser);
    request_window_redraw_for_browser(hub, browser_id);
}

const VMUX_LINK_HINTS_HIDE_JS: &str = concat!(
    "try{document.documentElement.removeAttribute('data-vmux-hint-precount');}catch(e){}",
    "try{window.__vmux_hints_cleanup&&window.__vmux_hints_cleanup();}catch(e){}",
);

fn link_hints_hide_execute(browser: &Browser) {
    let frames = link_hints_frame_list(browser);
    let code = CefString::from(VMUX_LINK_HINTS_HIDE_JS);
    let url = CefString::from("vmux://link-hints-clear");
    for f in frames {
        f.execute_java_script(Some(&code), Some(&url), 0);
    }
}

pub fn apply_link_hints_show_browser_events_system(
    mut events: EventReader<LinkHintsShowBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let Some(browser) = cef_browser_by_id(ev.browser_id) else {
                continue;
            };
            link_hints_show_execute(&browser, ev.browser_id, hub.0.as_ref());
            continue;
        }
        let mut task = LinkHintsShowPerformOnUiTask::new(ev.browser_id);
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_link_hints_show_browser_events_system: post_task failed"
            );
        }
    }
}

pub fn apply_link_hints_hide_browser_events_system(
    mut events: EventReader<LinkHintsHideBrowserEvent>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let Some(browser) = cef_browser_by_id(ev.browser_id) else {
                continue;
            };
            link_hints_hide_execute(&browser);
            continue;
        }
        let mut task = LinkHintsHidePerformOnUiTask::new(ev.browser_id);
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_link_hints_hide_browser_events_system: post_task failed"
            );
        }
    }
}

pub fn apply_link_hints_feed_key_deferred_browser_events_system(
    mut events: EventReader<LinkHintsFeedKeyDeferredBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            let outcome = link_hints_feed_key_on_ui(ev.browser_id, ev.ch, ev.prior_typed_len);
            if let Some(wid) =
                crate::browser::cef::osr::window_id_for_browser(hub.0.as_ref(), ev.browser_id)
            {
                crate::runtime::send_user_event(crate::runtime::UserEvent::App(
                    crate::runtime::AppEvent::LinkHintFeed(crate::runtime::LinkHintFeedEvent {
                        window_id: wid,
                        browser_id: ev.browser_id,
                        ch: ev.ch,
                        prior_typed_len: ev.prior_typed_len,
                        still_active: outcome.still_active,
                        hint_label_width: outcome.hint_label_width,
                    }),
                ));
            }
            continue;
        }
        let mut task =
            LinkHintsFeedKeyDeferredPerformOnUiTask::new(ev.browser_id, ev.ch, ev.prior_typed_len);
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_link_hints_feed_key_deferred_browser_events_system: post_task failed"
            );
        }
    }
}

wrap_task! {
    struct LinkHintsShowPerformOnUiTask {
        browser_id: i32,
    }
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let Some(hub) = crate::runtime::try_ffi_osr_index() else {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "LinkHintsShowPerformOnUiTask: foreign OSR index missing"
                );
                return;
            };
            for attempt in 0..32u32 {
                if let Some(browser) = cef_browser_by_id(self.browser_id) {
                    link_hints_show_execute(&browser, self.browser_id, hub.as_ref());
                    return;
                }
                if attempt + 1 < 32 {
                    crate::browser::cef::message_loop_pump(2);
                }
            }
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "LinkHintsShowPerformOnUiTask: no browser for browser_id={}",
                self.browser_id
            );
        }
    }
}

wrap_task! {
    struct LinkHintsHidePerformOnUiTask {
        browser_id: i32,
    }
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            if let Some(browser) = cef_browser_by_id(self.browser_id) {
                link_hints_hide_execute(&browser);
            }
        }
    }
}

wrap_task! {
    struct LinkHintsFeedKeyDeferredPerformOnUiTask {
        browser_id: i32,
        ch: char,
        prior_typed_len: usize,
    }
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::Input(
                    crate::runtime::ShellInputEvent::LinkHintsFeedKeyDeferredBrowser(
                        LinkHintsFeedKeyDeferredBrowserEvent {
                            browser_id: self.browser_id,
                            ch: self.ch,
                            prior_typed_len: self.prior_typed_len,
                        },
                    ),
                ),
            );
        }
    }
}

#[cfg(test)]
use bevy_ecs::prelude::{Resource, ResMut, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct LinkHintsLabelsIn {
    label_start: usize,
    label_total: usize,
}

#[cfg(test)]
#[derive(Resource, Default)]
struct LinkHintsLabelsOut(String);

#[cfg(test)]
fn link_hints_labels_populate_system(
    inp: Res<LinkHintsLabelsIn>,
    mut out: ResMut<LinkHintsLabelsOut>,
) {
    out.0 = vmux_link_hints_js_with_labels(inp.label_start, inp.label_total);
}

#[cfg(test)]
#[test]
fn vmux_link_hints_js_replaces_label_placeholders_via_system() {
    let mut world = World::default();
    world.insert_resource(LinkHintsLabelsIn {
        label_start: 7,
        label_total: 99,
    });
    world.insert_resource(LinkHintsLabelsOut::default());
    world
        .run_system_once(link_hints_labels_populate_system)
        .unwrap();
    let s = &world.resource::<LinkHintsLabelsOut>().0;
    assert!(
        !s.contains("__VMUX_LABEL"),
        "placeholders must be substituted: {s}"
    );
    assert!(s.contains('7') && s.contains("99"), "{s}");
}

#[cfg(test)]
#[derive(Resource, Default)]
struct LinkHintsPrecheckShadowDom(bool);

#[cfg(test)]
fn link_hints_shadow_dom_check_system(mut out: ResMut<LinkHintsPrecheckShadowDom>) {
    const PREC: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/link_hints_precount.js"
    ));
    out.0 = PREC.contains("shadowRoot") && VMUX_LINK_HINTS_JS_TEMPLATE.contains("shadowRoot");
}

#[cfg(test)]
#[test]
fn link_hints_scripts_pierce_shadow_dom_via_system() {
    let mut world = World::default();
    world.insert_resource(LinkHintsPrecheckShadowDom::default());
    world
        .run_system_once(link_hints_shadow_dom_check_system)
        .unwrap();
    assert!(
        world.resource::<LinkHintsPrecheckShadowDom>().0,
        "hint scan must recurse into shadow roots (e.g. google.com search UI)"
    );
}

#[cfg(test)]
#[derive(Resource, Default)]
struct LinkHintsPrecheckGoogle(bool);

#[cfg(test)]
fn link_hints_google_heuristics_check_system(mut out: ResMut<LinkHintsPrecheckGoogle>) {
    const PREC: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/link_hints_precount.js"
    ));
    out.0 = PREC.contains("L2AGLb")
        && PREC.contains("data-testid^=")
        && PREC.contains("countWithViewport");
}

#[cfg(test)]
#[test]
fn link_hints_scripts_cover_google_consent_heuristics_via_system() {
    let mut world = World::default();
    world.insert_resource(LinkHintsPrecheckGoogle::default());
    world
        .run_system_once(link_hints_google_heuristics_check_system)
        .unwrap();
    assert!(
        world.resource::<LinkHintsPrecheckGoogle>().0,
        "precount script must include Google consent heuristics"
    );
}
