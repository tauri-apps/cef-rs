//! List frames and run tiny JS (blur) across every frame of a browser.

use cef::{Browser, CefString, CefStringList, Frame, ImplBrowser as _, ImplFrame as _};

/// Stable frame order for multi-frame hints (main + subframes).
pub(crate) fn browser_all_frames(browser: &Browser) -> Vec<Frame> {
    let mut list = CefStringList::new();
    browser.frame_identifiers(Some(&mut list));
    let mut ids: Vec<String> = list.into_iter().collect();
    ids.sort();
    if !ids.is_empty() {
        return ids
            .into_iter()
            .filter_map(|id| {
                let cs = CefString::from(id.as_str());
                browser.frame_by_identifier(Some(&cs))
            })
            .collect();
    }
    browser.main_frame().into_iter().collect()
}

const VMUX_BLUR_ACTIVE_ELEMENT_JS: &str =
    r#"try{var a=document.activeElement;if(a&&typeof a.blur==='function')a.blur();}catch(e){}"#;

/// Blur `document.activeElement` in every frame (consent iframes / language `<button>` traps).
pub(crate) fn blur_active_element_all_frames(browser: &Browser) {
    let url = CefString::from("vmux://blur-active-element");
    let code = CefString::from(VMUX_BLUR_ACTIVE_ELEMENT_JS);
    for frame in browser_all_frames(browser) {
        frame.execute_java_script(Some(&code), Some(&url), 0);
    }
}
