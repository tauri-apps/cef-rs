//! Immediate visual feedback when quit begins (before CEF / OSR teardown finishes).

/// Slightly dim all application windows and show the busy cursor (macOS main thread only).
/// Idempotent for the lifetime of the process.
pub fn try_begin_quit_visual_feedback() {
    #[cfg(target_os = "macos")]
    try_begin_quit_visual_feedback_macos();
}

#[cfg(target_os = "macos")]
fn try_begin_quit_visual_feedback_macos() {
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::ClassType;
    use objc2::MainThreadMarker;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApp, NSCursor};

    static QUIT_FEEDBACK_APPLIED: AtomicBool = AtomicBool::new(false);

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if QUIT_FEEDBACK_APPLIED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let app = NSApp(mtm);
    let windows = app.windows();
    let n = windows.count();
    for i in 0..n {
        windows.objectAtIndex(i).setAlphaValue(0.88);
    }
    // `+[NSCursor busyArrowCursor]` is deprecated but still the clearest “quitting” affordance.
    let ptr: *mut NSCursor = unsafe { msg_send![NSCursor::class(), busyArrowCursor] };
    if let Some(c) = unsafe { Retained::retain_autoreleased(ptr) } {
        c.push();
    }
}
