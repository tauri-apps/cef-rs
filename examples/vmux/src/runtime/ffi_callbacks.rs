//! Winit proxy, pump scheduling, and GPU/OSR `OnceLock`s for code that cannot hold `&World` / `Res<…>`.
//!
//! Browser handle / lifecycle / attach mirrors live in [`crate::browser::cef::ffi`]. After CEF
//! `Startup`, GPU and OSR index match [`crate::browser::cef::GpuResource`] and
//! [`crate::browser::cef::ForeignOsrIndexResource`]; prefer those in Bevy `Update` systems.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use winit::event_loop::EventLoopProxy;

use crate::browser::cef::osr::ForeignOsrIndex;
use crate::browser::cef::renderer::SharedGpu;

use super::event::{AppEvent, UserEvent};

static WINIT_PROXY_FOR_FFI: OnceLock<EventLoopProxy<UserEvent>> = OnceLock::new();
/// Set when AppKit quit fires before [`register_winit_proxy_for_ffi`]; drained in
/// [`super::runner::run_winit`].
static PENDING_REQUEST_QUIT: AtomicBool = AtomicBool::new(false);
static GPU_FOR_FFI: OnceLock<Arc<SharedGpu>> = OnceLock::new();
static FFI_OSR_INDEX: OnceLock<Arc<ForeignOsrIndex>> = OnceLock::new();
static DEVICE_SCALE_FACTOR_FOR_FFI: OnceLock<Arc<Mutex<f32>>> = OnceLock::new();

/// Register the winit proxy for CEF / AppKit code paths that cannot access Bevy [`bevy_ecs::world::World`].
pub fn register_winit_proxy_for_ffi(proxy: EventLoopProxy<UserEvent>) {
    let _ = WINIT_PROXY_FOR_FFI.set(proxy);
}

pub fn schedule_cef_work(delay_ms: i64) {
    let Some(proxy) = WINIT_PROXY_FOR_FFI.get() else {
        return;
    };
    let deadline = Instant::now() + Duration::from_millis(delay_ms.max(0) as u64);
    let _ = proxy.send_event(UserEvent::App(AppEvent::ScheduleCefPump { deadline }));
}

pub fn request_quit() {
    if let Some(proxy) = WINIT_PROXY_FOR_FFI.get() {
        let ok = proxy
            .send_event(UserEvent::App(AppEvent::RequestQuit))
            .is_ok();
        crate::log::record_runtime_event(&format!("request_quit proxy_send ok={ok}"));
    } else {
        PENDING_REQUEST_QUIT.store(true, Ordering::Release);
        crate::log::record_runtime_event("request_quit no_proxy set_pending_request_quit");
    }
}

/// Returns true if a quit was pending and should become [`bevy_app::AppExit`] this runner turn.
pub fn take_pending_request_quit() -> bool {
    PENDING_REQUEST_QUIT.swap(false, Ordering::AcqRel)
}

pub fn send_user_event(event: UserEvent) {
    let Some(proxy) = WINIT_PROXY_FOR_FFI.get() else {
        return;
    };
    let _ = proxy.send_event(event);
}

pub fn register_gpu_runtime_for_ffi(
    gpu: Arc<SharedGpu>,
    osr_index: Arc<ForeignOsrIndex>,
    device_scale_factor: Arc<Mutex<f32>>,
) {
    let _ = GPU_FOR_FFI.set(gpu);
    let _ = FFI_OSR_INDEX.set(osr_index);
    let _ = DEVICE_SCALE_FACTOR_FOR_FFI.set(device_scale_factor);
}

/// OSR index + scale only (macOS: before first `NSWindow` + wgpu surface exist).
pub fn register_osr_index_and_scale_for_ffi(
    osr_index: Arc<ForeignOsrIndex>,
    device_scale_factor: Arc<Mutex<f32>>,
) {
    let _ = FFI_OSR_INDEX.set(osr_index);
    let _ = DEVICE_SCALE_FACTOR_FOR_FFI.set(device_scale_factor);
}

/// Call after [`register_osr_index_and_scale_for_ffi`] when the real [`SharedGpu`] exists.
pub fn register_gpu_only_for_ffi(gpu: Arc<SharedGpu>) {
    let _ = GPU_FOR_FFI.set(gpu);
}

fn exit_ffi_missing(what: &str) -> ! {
    eprintln!(
        "vmux FATAL: {what} (runtime ffi OnceLock empty — wrong init order or helper process)"
    );
    std::process::exit(79);
}

pub fn ffi_gpu() -> Arc<SharedGpu> {
    GPU_FOR_FFI
        .get()
        .cloned()
        .unwrap_or_else(|| exit_ffi_missing("GPU not registered for ffi"))
}

pub fn try_ffi_gpu() -> Option<Arc<SharedGpu>> {
    GPU_FOR_FFI.get().cloned()
}

pub fn ffi_osr_index() -> Arc<ForeignOsrIndex> {
    FFI_OSR_INDEX
        .get()
        .cloned()
        .unwrap_or_else(|| exit_ffi_missing("OSR index not registered for ffi"))
}

pub fn try_ffi_osr_index() -> Option<Arc<ForeignOsrIndex>> {
    FFI_OSR_INDEX.get().cloned()
}

pub fn ffi_device_scale_factor() -> Arc<Mutex<f32>> {
    DEVICE_SCALE_FACTOR_FOR_FFI
        .get()
        .cloned()
        .unwrap_or_else(|| exit_ffi_missing("device_scale_factor not registered for ffi"))
}
