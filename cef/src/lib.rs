#![doc = include_str!("../README.md")]

pub mod args;
pub mod rc;
pub mod string;
pub mod window_info;
pub mod wrapper;

#[cfg(target_os = "macos")]
pub mod application_mac;

#[cfg(target_os = "macos")]
pub mod library_loader;

#[cfg(target_os = "macos")]
pub mod sandbox;

#[cfg(feature = "accelerated_osr")]
pub mod osr_texture_import;

#[cfg(feature = "build-util")]
pub mod build_util;

#[cfg(feature = "resources")]
pub mod resources;

#[rustfmt::skip]
mod bindings;
pub use bindings::*;

pub use rc::Rc as _;

pub use cef_dll_sys as sys;

#[cfg(all(
    not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
    feature = "accelerated_osr"
))]
compile_error!("accelerated_osr not supported on this platform");

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

#[cfg(test)]
fn test_init_cef() {
    use std::{ptr, sync::Once};

    static INIT: Once = Once::new();
    INIT.call_once(|| {
        #[cfg(target_os = "macos")]
        unsafe {
            use std::{ffi::CString, os::unix::ffi::OsStrExt};

            let cef_dir = sys::get_cef_dir().expect("CEF not found");
            let framework_dir = cef_dir
                .join(sys::FRAMEWORK_PATH)
                .canonicalize()
                .expect("failed to get framework path");
            let framework_dir =
                CString::new(framework_dir.as_os_str().as_bytes()).expect("invalid path");

            assert_eq!(sys::cef_load_library(framework_dir.as_ptr().cast()), 1);
        }

        assert_eq!(initialize(None, None, None, ptr::null_mut()), 0);

        let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    });
}
