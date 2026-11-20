//! Vimium-like UX modes: find bar, CEF find, visual banner, yank helpers.

use cef::{Browser, CefString, ImplBrowser as _, ImplBrowserHost as _, ImplFrame as _};

const FIND_UI_BOOTSTRAP: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/find_ui.js"));
const VISUAL_HINT_BOOTSTRAP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/visual_hint.js"
));

fn escape_js_single_quoted(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\'' => o.push_str("\\'"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 32 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn exec_js(browser: &Browser, code: &str) {
    let Some(frame) = browser.focused_frame().or_else(|| browser.main_frame()) else {
        return;
    };
    let code = CefString::from(code);
    let url = CefString::from("vmux://vimium-modes");
    frame.execute_java_script(Some(&code), Some(&url), 0);
}

pub fn find_ui_show(browser: &Browser) {
    exec_js(browser, FIND_UI_BOOTSTRAP);
}

pub fn find_ui_set_query(browser: &Browser, query: &str) {
    let esc = escape_js_single_quoted(query);
    exec_js(
        browser,
        &format!(
            "try{{if(window.__vmux_find_ui_set)window.__vmux_find_ui_set('{esc}');}}catch(e){{}}"
        ),
    );
}

pub fn find_ui_hide(browser: &Browser) {
    exec_js(
        browser,
        "try{if(window.__vmux_find_ui_remove)window.__vmux_find_ui_remove();}catch(e){}",
    );
}

pub fn visual_hint_show(browser: &Browser) {
    exec_js(browser, VISUAL_HINT_BOOTSTRAP);
}

pub fn visual_hint_hide(browser: &Browser) {
    exec_js(
        browser,
        "try{if(window.__vmux_visual_remove)window.__vmux_visual_remove();}catch(e){}",
    );
}

pub fn cef_find(browser: &Browser, query: &str, forward: bool, find_next: bool) {
    let Some(host) = browser.host() else {
        return;
    };
    let s = CefString::from(query);
    host.find(Some(&s), forward as i32, 0, find_next as i32);
}

pub fn cef_stop_finding(browser: &Browser, clear_selection: bool) {
    let Some(host) = browser.host() else {
        return;
    };
    host.stop_finding(clear_selection as i32);
}

pub fn yank_page_url(browser: &Browser) {
    exec_js(
        browser,
        r#"try{var u=location.href;if(navigator.clipboard&&navigator.clipboard.writeText)navigator.clipboard.writeText(u);}catch(e){}"#,
    );
}

pub fn yank_selection(browser: &Browser) {
    exec_js(
        browser,
        r#"try{var t=window.getSelection&&window.getSelection().toString()||"";if(t&&navigator.clipboard&&navigator.clipboard.writeText)navigator.clipboard.writeText(t);}catch(e){}"#,
    );
}
