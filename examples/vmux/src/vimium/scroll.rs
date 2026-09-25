use cef::{Browser, CefString, ImplBrowser as _, ImplFrame as _};

/// First `j` / `k` tap: smooth step.
const LINE_PX: i32 = 64;
/// OS key-repeat while holding: instant scroll, larger step (no stacked smooth animations).
const LINE_PX_HOLD: i32 = 160;

fn exec_js(browser: &Browser, code: &str) {
    let Some(frame) = browser.focused_frame().or_else(|| browser.main_frame()) else {
        return;
    };
    let code = CefString::from(code);
    let url = CefString::from("vmux://vimium-scroll");
    frame.execute_java_script(Some(&code), Some(&url), 0);
}

pub fn scroll_line_down(browser: &Browser, key_repeat: bool) {
    if key_repeat {
        exec_js(
            browser,
            &format!("window.scrollBy({{top:{LINE_PX_HOLD},behavior:'auto'}});"),
        );
    } else {
        exec_js(
            browser,
            &format!("window.scrollBy({{top:{LINE_PX},behavior:'smooth'}});"),
        );
    }
}

pub fn scroll_line_up(browser: &Browser, key_repeat: bool) {
    if key_repeat {
        exec_js(
            browser,
            &format!(
                "window.scrollBy({{top:{},behavior:'auto'}});",
                -LINE_PX_HOLD
            ),
        );
    } else {
        exec_js(
            browser,
            &format!("window.scrollBy({{top:{},behavior:'smooth'}});", -LINE_PX),
        );
    }
}

pub fn scroll_page_down(browser: &Browser, key_repeat: bool) {
    if key_repeat {
        exec_js(
            browser,
            "var h=window.innerHeight||document.documentElement.clientHeight||400,d=Math.max(1,Math.floor(h*0.9));window.scrollBy({top:d,behavior:'auto'});",
        );
    } else {
        exec_js(
            browser,
            "var h=window.innerHeight||document.documentElement.clientHeight||400,d=Math.max(1,Math.floor(h*0.9));window.scrollBy({top:d,behavior:'smooth'});",
        );
    }
}

pub fn scroll_page_up(browser: &Browser, key_repeat: bool) {
    if key_repeat {
        exec_js(
            browser,
            "var h=window.innerHeight||document.documentElement.clientHeight||400,d=Math.max(1,Math.floor(h*0.9));window.scrollBy({top:-d,behavior:'auto'});",
        );
    } else {
        exec_js(
            browser,
            "var h=window.innerHeight||document.documentElement.clientHeight||400,d=Math.max(1,Math.floor(h*0.9));window.scrollBy({top:-d,behavior:'smooth'});",
        );
    }
}

pub fn scroll_top(browser: &Browser) {
    exec_js(browser, "window.scrollTo({top:0,behavior:'smooth'});");
}

pub fn scroll_bottom(browser: &Browser) {
    exec_js(
        browser,
        "var e=document.scrollingElement||document.documentElement||document.body,y=(e&&e.scrollHeight)||0;window.scrollTo({top:y,behavior:'smooth'});",
    );
}
