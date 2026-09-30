//! Back/forward history navigation helpers for browser apply systems.

use cef::rc::Rc;
use cef::{
    Browser, CefString, ImplBrowser, ImplBrowserHost, ImplFrame, ImplNavigationEntry,
    ImplNavigationEntryVisitor, NavigationEntry, NavigationEntryVisitor, WrapNavigationEntryVisitor,
    wrap_navigation_entry_visitor,
};
use std::sync::{Arc, Mutex};

pub(crate) fn visible_navigation_url(browser: &Browser) -> Option<String> {
    let host = browser.host()?;
    let e = host.visible_navigation_entry()?;
    if e.is_valid() == 0 {
        return None;
    }
    let u = e.url();
    Some(CefString::from(&u).to_string())
}

wrap_navigation_entry_visitor! {
    struct NavEntriesCollector {
        entries: Arc<Mutex<Vec<(String, i32, bool)>>>,
    }

    impl NavigationEntryVisitor {
        fn visit(
            &self,
            entry: Option<&mut NavigationEntry>,
            current: std::os::raw::c_int,
            index: std::os::raw::c_int,
            _total: std::os::raw::c_int,
        ) -> std::os::raw::c_int {
            let Some(entry) = entry else {
                return 0;
            };
            if entry.is_valid() == 0 {
                return 0;
            }
            let u = entry.url();
            let url = CefString::from(&u).to_string();
            if let Ok(mut rows) = self.entries.lock() {
                rows.push((url, index, current != 0));
            }
            0
        }
    }
}

pub(crate) fn history_url_adjacent(browser: &Browser, back: bool) -> Option<String> {
    let host = browser.host()?;
    let visible = visible_navigation_url(browser);
    let entries = Arc::new(Mutex::new(Vec::<(String, i32, bool)>::new()));
    let mut visitor = NavEntriesCollector::new(Arc::clone(&entries));
    host.navigation_entries(Some(&mut visitor), 0);
    let mut rows = entries.lock().ok()?;
    if rows.len() < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    let cur_pos = rows.iter().position(|(_, _, is_cur)| *is_cur).or_else(|| {
        let v = visible.as_deref()?;
        rows.iter().position(|(u, _, _)| u == v)
    })?;
    let target_pos = if back {
        cur_pos.checked_sub(1)?
    } else if cur_pos + 1 < rows.len() {
        cur_pos + 1
    } else {
        return None;
    };
    Some(rows[target_pos].0.clone())
}

pub(crate) fn apply_history_navigation(browser: &Browser, go_forward: bool) {
    let before = visible_navigation_url(browser);
    if go_forward {
        browser.go_forward();
    } else {
        browser.go_back();
    }
    crate::browser::cef::message_loop_pump(40);
    let after = visible_navigation_url(browser);
    let stuck = match (&before, &after) {
        (Some(b), Some(a)) => b == a,
        (Some(_), None) => true,
        _ => false,
    };
    if stuck {
        let back = !go_forward;
        if let Some(url) = history_url_adjacent(browser, back) {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                "apply_history_navigation: URL unchanged after go_{}; loading {:?}",
                if go_forward { "forward" } else { "back" },
                url
            );
            let u = CefString::from(url.as_str());
            if let Some(frame) = browser.main_frame() {
                frame.load_url(Some(&u));
                crate::browser::cef::message_loop_pump(40);
            }
        }
    }
}
