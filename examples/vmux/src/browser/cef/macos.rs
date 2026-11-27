use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, define_class, extern_methods, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObjectProtocol},
};
use objc2_app_kit::{NSApp, NSApplication, NSEvent};

/// Bevy-driven graceful shutdown request: close browsers and let the runner drain.
fn request_graceful_shutdown() {
    crate::runtime::request_quit();
}

#[derive(Default)]
pub struct VmuxApplicationIvars {
    handling_send_event: std::cell::Cell<Bool>,
}

define_class!(
    #[unsafe(super(NSApplication))]
    #[ivars = VmuxApplicationIvars]
    // Must match `NSPrincipalClass` / `[package.metadata.cef.bundle] principal_class` in the bundled
    // `Info.plist`. The objc2 default name is `module_path::VmuxApplication` + version, which
    // AppKit will not resolve from the plist string `VmuxApplication`.
    #[name = "VmuxApplication"]
    pub struct VmuxApplication;

    impl VmuxApplication {
        /// Required for Chromium macOS embedding: coordinate event delivery with CEF (`CrApp`).
        /// Matches [`examples/cefsimple/src/mac/mod.rs`](../../../../cefsimple/src/mac/mod.rs).
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            // Always forward to `NSApplication` / Chromium (`CrApp`). Skipping `super` for Cmd+Q
            // breaks CEF embedding and can leave OSR black with a stuck event pipeline.
            let was = self.ivars().handling_send_event.get();
            if was == Bool::NO {
                self.ivars().handling_send_event.set(Bool::YES);
            }
            let _: () = msg_send![super(self), sendEvent: event];
            if was == Bool::NO {
                self.ivars().handling_send_event.set(Bool::NO);
            }
        }

        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: &AnyObject) {
            crate::browser::cef::quit::try_begin_quit_visual_feedback();
            request_graceful_shutdown();
        }
    }

    unsafe impl CrAppControlProtocol for VmuxApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn _set_handling_send_event(&self, handling_send_event: Bool) {
            self.ivars().handling_send_event.set(handling_send_event);
        }
    }

    unsafe impl CrAppProtocol for VmuxApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn _is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for VmuxApplication {}
);

impl VmuxApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared_application() -> Retained<Self>;
    }
}

pub fn setup_vmux_application() {
    // Register the ObjC class before `sharedApplication` (objc2: registration happens on `class()`).
    let _ = VmuxApplication::class();
    let _ = VmuxApplication::shared_application();
}

/// Call after `EventLoop::build` (or any code that may touch AppKit). If this fails, something
/// constructed `NSApplication` before [`setup_vmux_application`]; Chromium requires the CEF
/// `NSApplication` subclass (see cefsimple).
#[cfg(debug_assertions)]
pub fn debug_assert_vmux_is_frontmost_app_subclass() {
    let mtm = MainThreadMarker::new().expect("vmux: main-thread marker");
    let app = NSApp(mtm);
    assert!(
        app.isKindOfClass(VmuxApplication::class()),
        "vmux: NSApplication is not VmuxApplication — call setup_vmux_application() before Bevy, winit, or other AppKit use"
    );
}
