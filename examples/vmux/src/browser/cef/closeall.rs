//! Close every tracked browser on the CEF UI thread (no `browser::system` dependency for call sites).

use cef::{ImplBrowser as _, ImplBrowserHost as _, ThreadId, currently_on};

use super::ffi::{
    ffi_browser_cef_attach, ffi_browser_cef_handles, ffi_browser_close_guards,
    ffi_browser_lifecycle,
};

/// Close every browser from the CEF UI thread (`close_browser` is not safe from arbitrary threads).
pub fn close_all_browsers_on_ui_thread(force_close: bool) {
    if currently_on(ThreadId::UI) == 0 {
        crate::log::record_runtime_event(
            "close_all_browsers_on_ui_thread skip not_on_cef_ui_thread",
        );
        return;
    }
    if !force_close
        && ffi_browser_lifecycle()
            .lock()
            .map(|g| g.is_closing)
            .unwrap_or(false)
    {
        crate::log::record_runtime_event(
            "close_all_browsers_on_ui_thread skip already_closing_non_force",
        );
        return;
    }

    let has_cef_attach = ffi_browser_cef_attach().is_some();
    if has_cef_attach {
        if let Ok(mut g) = ffi_browser_close_guards().lock() {
            g.bypass_windowless_do_close_guard = true;
        }
        if force_close {
            if let Ok(mut g) = ffi_browser_lifecycle().lock() {
                g.is_closing = true;
            }
        }
    }

    let mut browsers = ffi_browser_cef_handles()
        .lock()
        .map(|g| g.values_cloned())
        .unwrap_or_default();
    if browsers.is_empty() {
        if let Some(attach) = ffi_browser_cef_attach() {
            if let Ok(ws) = attach.windows_store.lock() {
                browsers = ws.values().map(|e| e.browser.clone()).collect();
            }
        }
    }

    if browsers.is_empty() {
        crate::log::record_runtime_event(&format!(
            "close_all_browsers_on_ui_thread force_close={force_close} count=0 push_shutdown_unstick",
        ));
        if force_close {
            if let Ok(mut g) = ffi_browser_lifecycle().lock() {
                g.is_closing = false;
            }
            crate::runtime::send_user_event(crate::runtime::UserEvent::App(
                crate::runtime::AppEvent::ShutdownRequested,
            ));
        }
        return;
    }

    crate::log::record_runtime_event(&format!(
        "close_all_browsers_on_ui_thread force_close={force_close} count={}",
        browsers.len()
    ));

    for browser in browsers {
        if let Some(browser_host) = browser.host() {
            browser_host.close_browser(force_close.into());
        }
    }

    if has_cef_attach && !force_close {
        if let Ok(mut g) = ffi_browser_close_guards().lock() {
            g.bypass_windowless_do_close_guard = false;
        }
    }
}
