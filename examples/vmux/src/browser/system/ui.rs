//! Paint / redraw / title sync helpers shared by browser [`super`] systems.

use cef::{
    Browser, CefStringUtf8, CefStringUtf16, ImplBrowser, ImplBrowserHost, ImplNavigationEntry,
};

use crate::browser::cef::ffi::{ffi_browser_cef_attach, ffi_browser_cef_handles};
use crate::browser::cef::osr::ForeignOsrIndex;

pub(crate) fn repaint_full_geometry(browser: &Browser) {
    let Some(host) = browser.host() else {
        return;
    };
    host.was_resized();
    host.notify_screen_info_changed();
    host.invalidate(cef::PaintElementType::VIEW);
    #[cfg(feature = "accelerated_osr")]
    host.send_external_begin_frame();
}

pub(crate) fn request_window_redraw_for_browser(index: &ForeignOsrIndex, browser_id: i32) {
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

pub(crate) fn sync_title_from_visible_navigation(browser: &Browser) {
    if ffi_browser_cef_attach().is_none() {
        return;
    }
    let bid = browser.identifier();
    let Some(host) = browser.host() else {
        return;
    };
    let Some(entry) = host.visible_navigation_entry() else {
        return;
    };
    if entry.is_valid() == 0 {
        return;
    }
    let title_raw = entry.title();
    let mut title_str = CefStringUtf8::from(&CefStringUtf16::from(&title_raw)).to_string();
    if title_str.is_empty() {
        let disp = entry.display_url();
        title_str = CefStringUtf8::from(&CefStringUtf16::from(&disp)).to_string();
    }
    if !title_str.is_empty() {
        crate::browser::cef::titles::push_title(bid, title_str);
    }
}

pub(crate) fn navigation_repaint_pass_on_ui(index: &ForeignOsrIndex, browser_id: i32) {
    debug_assert_ne!(cef::currently_on(cef::ThreadId::UI), 0);
    let Some(browser) = ffi_browser_cef_handles()
        .lock()
        .ok()
        .and_then(|g| g.get(browser_id).cloned())
    else {
        return;
    };
    crate::browser::cef::osr::reset_paint_redraw_throttle(index, browser_id);
    repaint_full_geometry(&browser);
    crate::browser::cef::message_loop_pump(14);
    request_window_redraw_for_browser(index, browser_id);
}
