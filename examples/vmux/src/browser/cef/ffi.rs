//! `OnceLock` handles for browser / CEF shell state when code cannot hold `Res<…>` (CEF UI thread,
//! `RenderHandler`, etc.). The same `Arc`s are registered as Bevy resources in
//! [`crate::browser::BrowserPlugin`] (Bevy resources + [`register_browser_runtime_for_ffi`]); [`CefAttach`] is set from `cef` startup.

use std::sync::{Arc, Mutex, OnceLock};

use crate::browser::cef::entity::CefBrowserHandlesInner;
use crate::browser::cef::lifecycle::{BrowserCloseGuardsInner, BrowserLifecycleInner};
use crate::browser::cef::osr::CefAttach;

static BROWSER_CEF_ATTACH_FOR_FFI: OnceLock<Option<CefAttach>> = OnceLock::new();
static BROWSER_LIFECYCLE_FOR_FFI: OnceLock<Arc<Mutex<BrowserLifecycleInner>>> = OnceLock::new();
static BROWSER_CEF_HANDLES_FOR_FFI: OnceLock<Arc<Mutex<CefBrowserHandlesInner>>> = OnceLock::new();
static BROWSER_CLOSE_GUARDS_FOR_FFI: OnceLock<Arc<Mutex<BrowserCloseGuardsInner>>> =
    OnceLock::new();

fn exit_missing(what: &str) -> ! {
    eprintln!("vmux FATAL: {what} (cef ffi OnceLock empty — wrong init order or helper process)");
    std::process::exit(79);
}

pub fn register_browser_runtime_for_ffi(
    cef_handles: Arc<Mutex<CefBrowserHandlesInner>>,
    lifecycle: Arc<Mutex<BrowserLifecycleInner>>,
    close_guards: Arc<Mutex<BrowserCloseGuardsInner>>,
) {
    let _ = BROWSER_CEF_HANDLES_FOR_FFI.set(cef_handles);
    let _ = BROWSER_LIFECYCLE_FOR_FFI.set(lifecycle);
    let _ = BROWSER_CLOSE_GUARDS_FOR_FFI.set(close_guards);
}

pub fn register_cef_attach_for_ffi(cef_attach: Option<CefAttach>) {
    let _ = BROWSER_CEF_ATTACH_FOR_FFI.set(cef_attach);
}

pub fn ffi_browser_cef_attach() -> Option<CefAttach> {
    BROWSER_CEF_ATTACH_FOR_FFI.get().cloned().flatten()
}

pub fn ffi_browser_lifecycle() -> Arc<Mutex<BrowserLifecycleInner>> {
    BROWSER_LIFECYCLE_FOR_FFI
        .get()
        .cloned()
        .unwrap_or_else(|| exit_missing("browser lifecycle not registered"))
}

pub fn ffi_browser_cef_handles() -> Arc<Mutex<CefBrowserHandlesInner>> {
    BROWSER_CEF_HANDLES_FOR_FFI
        .get()
        .cloned()
        .unwrap_or_else(|| exit_missing("browser CEF handles not registered"))
}

pub fn ffi_browser_close_guards() -> Arc<Mutex<BrowserCloseGuardsInner>> {
    BROWSER_CLOSE_GUARDS_FOR_FFI
        .get()
        .cloned()
        .unwrap_or_else(|| exit_missing("browser close guards not registered"))
}
