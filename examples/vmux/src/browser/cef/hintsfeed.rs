//! Synchronous link-hint key feed on the CEF UI thread ([`link_hints_feed_key_on_ui`]).

use std::sync::{Arc, Mutex};

use cef::rc::Rc;
use cef::{
    Browser, CefString, CefStringUtf8, CefStringUtf16, Domdocument, Domvisitor, Frame,
    ImplDomdocument, ImplDomnode, ImplDomvisitor, ImplFrame, ThreadId, WrapDomvisitor,
    currently_on, wrap_domvisitor,
};

use crate::browser::cef::ffi::ffi_browser_cef_attach;
use crate::browser::cef::frames::browser_all_frames;
use crate::browser::cef::lookup::cef_browser_by_id;
use crate::browser::cef::osr::ForeignOsrIndex;

/// After [`link_hints_feed_key_on_ui`]: whether the hint session should stay armed in Rust.
/// `data-vmux-hints` on `<html>` holds the fixed label width (stringified integer); when a DOM read
/// lags after a key, Rust uses [`link_hints_feed_key_on_ui`]'s `prior_typed_len` plus cached width.
/// Typed prefix is tracked in Rust (`VimiumState`) only.
#[derive(Debug, Clone)]
pub struct LinkHintsFeedOutcome {
    pub still_active: bool,
    /// Fixed code length for this page (from `data-vmux-hints` when the overlay is present; else best known).
    pub hint_label_width: u8,
}

impl Default for LinkHintsFeedOutcome {
    fn default() -> Self {
        Self {
            still_active: false,
            hint_label_width: 1,
        }
    }
}

pub(crate) fn link_hints_frame_list(browser: &Browser) -> Vec<Frame> {
    browser_all_frames(browser)
}

fn request_window_redraw_for_browser(index: &ForeignOsrIndex, browser_id: i32) {
    let Some(ref attach) = ffi_browser_cef_attach() else {
        return;
    };
    let Some(wid) = crate::browser::cef::osr::window_id_for_browser(index, browser_id) else {
        return;
    };
    if let Ok(mut ws) = attach.windows_store.lock() {
        if let Some(entry) = ws.get_mut(&wid) {
            entry.surface.window.request_redraw();
        }
    }
}

wrap_domvisitor! {
    struct LinkHintsSessionDomVisitor {
        out: Arc<Mutex<LinkHintsFeedOutcome>>,
    }

    impl Domvisitor {
        fn visit(&self, document: Option<&mut Domdocument>) {
            let (active, width) = match document {
                None => (false, 1u8),
                Some(doc) => doc
                    .document()
                    .map(|root| {
                        let hints = CefString::from("data-vmux-hints");
                        let active = root.has_element_attribute(Some(&hints)) != 0;
                        let width = if active {
                            let raw = root.element_attribute(Some(&hints));
                            let s = CefStringUtf8::from(&CefStringUtf16::from(&raw)).to_string();
                            s.trim().parse::<u8>().unwrap_or(2).clamp(1, 32)
                        } else {
                            1u8
                        };
                        (active, width)
                    })
                    .unwrap_or((false, 1u8)),
            };
            if let Ok(mut g) = self.out.lock() {
                if active {
                    g.still_active = true;
                    g.hint_label_width = g.hint_label_width.max(width);
                }
            }
        }
    }
}

fn link_hints_read_session_on_frame(frame: &Frame) -> LinkHintsFeedOutcome {
    let out = Arc::new(Mutex::new(LinkHintsFeedOutcome::default()));
    let mut visitor = LinkHintsSessionDomVisitor::new(Arc::clone(&out));
    frame.visit_dom(Some(&mut visitor));
    out.lock().ok().map(|g| g.clone()).unwrap_or_default()
}

pub(crate) fn link_hints_read_session_with_browser(browser: &Browser) -> LinkHintsFeedOutcome {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);
    let mut merged = LinkHintsFeedOutcome::default();
    for frame in link_hints_frame_list(browser) {
        let snap = link_hints_read_session_on_frame(&frame);
        if snap.still_active {
            merged.still_active = true;
            merged.hint_label_width = merged.hint_label_width.max(snap.hint_label_width);
        }
    }
    merged
}

fn link_hints_read_session_on_ui(browser_id: i32) -> LinkHintsFeedOutcome {
    let Some(browser) = cef_browser_by_id(browser_id) else {
        return LinkHintsFeedOutcome::default();
    };
    link_hints_read_session_with_browser(&browser)
}

fn link_hints_finalize_feed_outcome(
    snap: LinkHintsFeedOutcome,
    prior_typed_len: usize,
) -> LinkHintsFeedOutcome {
    let w_from_dom = snap.hint_label_width.max(1);
    if snap.still_active {
        return LinkHintsFeedOutcome {
            still_active: true,
            hint_label_width: w_from_dom,
        };
    }
    let w_cached = 2u8.max(1);
    let typed_len_after = prior_typed_len.saturating_add(1);
    let still = typed_len_after < w_cached as usize;
    LinkHintsFeedOutcome {
        still_active: still,
        hint_label_width: w_cached,
    }
}

/// Synchronous link-hint key feed on the CEF UI thread (see [`link_hints_feed_key_on_ui`]).
pub fn link_hints_feed_key_on_ui(
    browser_id: i32,
    ch: char,
    prior_typed_len: usize,
) -> LinkHintsFeedOutcome {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);
    if !ch.is_ascii_lowercase() {
        return link_hints_read_session_on_ui(browser_id);
    }
    let Some(browser) = cef_browser_by_id(browser_id) else {
        return LinkHintsFeedOutcome::default();
    };
    let code = format!(
        "try{{if(typeof window.__vmux_hints_feed==='function')window.__vmux_hints_feed('{}');}}catch(e){{}}",
        ch
    );
    let code = CefString::from(code.as_str());
    let url = CefString::from("vmux://link-hints-feed");
    for frame in link_hints_frame_list(&browser) {
        frame.execute_java_script(Some(&code), Some(&url), 0);
    }
    crate::browser::cef::message_loop_pump(12);
    if let Some(h) = crate::runtime::try_ffi_osr_index() {
        request_window_redraw_for_browser(h.as_ref(), browser_id);
    }
    let snap = link_hints_read_session_with_browser(&browser);
    link_hints_finalize_feed_outcome(snap, prior_typed_len)
}
