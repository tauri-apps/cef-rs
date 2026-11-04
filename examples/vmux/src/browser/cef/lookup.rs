//! Resolve [`cef::Browser`] by numeric id (OSR `windows_store` + `CefBrowserHandles`).

use cef::{Browser, ImplBrowser as _};

use super::ffi::{ffi_browser_cef_attach, ffi_browser_cef_handles};

/// Prefer **`windows_store`** (OSR), then tracked handles. See `browser::cef::entity` for the model.
pub fn cef_browser_by_id(browser_id: i32) -> Option<Browser> {
    let attach = ffi_browser_cef_attach();
    let from_store = attach.as_ref().and_then(|a| {
        a.windows_store.lock().ok().and_then(|ws| {
            ws.values()
                .find(|e| e.browser.identifier() == browser_id)
                .map(|e| e.browser.clone())
        })
    });
    from_store.or_else(|| {
        ffi_browser_cef_handles()
            .lock()
            .ok()
            .and_then(|g| g.get(browser_id).cloned())
    })
}
