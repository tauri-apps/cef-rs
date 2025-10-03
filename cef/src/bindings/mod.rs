#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod x86_64_unknown_linux_gnu;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use x86_64_unknown_linux_gnu::*;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod aarch64_unknown_linux_gnu;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub use aarch64_unknown_linux_gnu::*;

#[cfg(all(target_os = "linux", target_arch = "arm"))]
mod arm_unknown_linux_gnueabi;
#[cfg(all(target_os = "linux", target_arch = "arm"))]
pub use arm_unknown_linux_gnueabi::*;

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
mod x86_64_pc_windows_msvc;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub use x86_64_pc_windows_msvc::*;

#[cfg(all(target_os = "windows", target_arch = "x86"))]
mod i686_pc_windows_msvc;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
pub use i686_pc_windows_msvc::*;

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
mod aarch64_pc_windows_msvc;
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub use aarch64_pc_windows_msvc::*;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
mod x86_64_apple_darwin;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub use x86_64_apple_darwin::*;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod aarch64_apple_darwin;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use aarch64_apple_darwin::*;

#[cfg(test)]
mod test {
    use super::*;
    use crate::{rc::*, sys};
    use std::{cell::RefCell, ptr};

    #[derive(Default)]
    struct CallInfo {
        extra_info: RefCell<Option<DictionaryValue>>,
    }

    struct TestLifeSpanHandler {
        object: *mut RcImpl<sys::_cef_life_span_handler_t, Self>,
        call_info: std::rc::Rc<CallInfo>,
    }

    impl ImplLifeSpanHandler for TestLifeSpanHandler {
        fn get_raw(&self) -> *mut sys::_cef_life_span_handler_t {
            self.object.cast()
        }

        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            _target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let extra_info = extra_info.expect("extra_info is required");
            *extra_info = self.call_info.extra_info.take();
            1
        }
    }

    impl WrapLifeSpanHandler for TestLifeSpanHandler {
        fn wrap_rc(&mut self, object: *mut RcImpl<sys::_cef_life_span_handler_t, Self>) {
            self.object = object;
        }
    }

    impl TestLifeSpanHandler {
        fn new(call_info: std::rc::Rc<CallInfo>) -> LifeSpanHandler {
            LifeSpanHandler::new(Self {
                object: std::ptr::null_mut(),
                call_info,
            })
        }
    }

    impl Clone for TestLifeSpanHandler {
        fn clone(&self) -> Self {
            let object = unsafe {
                let rc_impl = &mut *self.object;
                rc_impl.interface.add_ref();
                self.object
            };

            Self {
                object,
                call_info: self.call_info.clone(),
            }
        }
    }

    impl Rc for TestLifeSpanHandler {
        fn as_base(&self) -> &sys::cef_base_ref_counted_t {
            unsafe {
                let base = &*self.object;
                std::mem::transmute(&base.cef_object)
            }
        }
    }

    #[test]
    fn dictionary_value_out_param() {
        #[cfg(target_os = "macos")]
        unsafe {
            use std::{os::unix::ffi::OsStrExt, CString};

            let cef_dir = sys::get_cef_dir().expect("CEF not found");
            let framework_dir = cef_dir
                .join(sys::FRAMEWORK_PATH)
                .canonicalize()
                .expect("failed to get framework path");
            let framework_dir =
                CString::new(framework_dir.as_os_str().as_bytes()).expect("invalid path");

            assert_eq!(cef_load_library(framework_dir.as_ptr().cast()), 1);
        }

        assert_eq!(initialize(None, None, None, ptr::null_mut()), 0);

        let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

        let call_info = std::rc::Rc::new(CallInfo::default());
        let extra_info = dictionary_value_create().expect("failed to create dictionary");
        let test_key = CefString::from("testKey");
        let test_value = CefString::from("testValue");
        extra_info.set_string(Some(&test_key), Some(&test_value));
        *call_info.extra_info.borrow_mut() = Some(extra_info);
        let mut extra_info = None;

        let handler = TestLifeSpanHandler::new(call_info);
        assert_eq!(
            1,
            handler.on_before_popup(
                None,
                None,
                1,
                None,
                None,
                sys::cef_window_open_disposition_t::CEF_WOD_CURRENT_TAB.into(),
                0,
                None,
                None,
                None,
                None,
                Some(&mut extra_info),
                None,
            )
        );
        let extra_info = extra_info.as_ref().unwrap();
        assert_eq!(
            "testValue",
            CefString::from(&extra_info.string(Some(&test_key))).to_string()
        );
    }
}
