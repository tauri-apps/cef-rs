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
    use crate::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct CallInfo {
        extra_info: RefCell<Option<DictionaryValue>>,
    }

    wrap_life_span_handler! {
        struct TestLifeSpanHandler {
            call_info: std::rc::Rc<CallInfo>,
        }

        impl LifeSpanHandler {
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
    }

    #[test]
    fn dictionary_value_out_param() {
        test_init_cef();

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
