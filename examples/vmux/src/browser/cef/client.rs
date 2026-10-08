//! CEF `Client`, display/load handlers, and UI-thread `post_task` bridges (navigation, link hints, …).
//! Call sites use Bevy events and [`crate::browser::cef::lookup::cef_browser_by_id`] where a handle
//! is needed on the UI thread (e.g. editable-focus probe in [`crate::browser::cef::focus`]).

use ::cef::ImplBrowser as _;
use ::cef::*;

wrap_client! {
    pub struct BrowserClient {
        render: RenderHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            // `examples/osr` client is render-only; vmux display/load run extra UI-thread work during
            // first frames. Address/title only (no `on_cursor_change`: winit + windows_store from CEF thread).
            Some(BrowserDisplayHandlerMacOsr::new())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            // No Rust `LifeSpanHandler` — avoids traps unwinding from `on_after_created`. Attach:
            // [`OsrHostState::macos_poll_cef_browser_attach`] after async create + pumps.
            None
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(BrowserLoadHandler::new())
        }
    }
}

wrap_display_handler! {
    struct BrowserDisplayHandlerMacOsr {}

    impl DisplayHandler {
        fn on_address_change(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            let Some(browser_id) = browser.as_ref().map(|b| b.identifier()) else {
                return;
            };
            let url_str = url.map(CefString::to_string);
            if let Some(ref s) = url_str {
                if s.contains("google.com") {
                    bevy_log::info!(
                        target: "vmux",
                        pid = std::process::id(),
                        "proof: cef_navigated_url_has_google browser_id={browser_id} url={s}"
                    );
                }
            }
            crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                crate::runtime::CefEvent::AddressChangedBrowser(
                    crate::browser::event::AddressChangedBrowserEvent {
                        browser_id,
                        url: url_str,
                    },
                ),
            ));
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let Some(browser_id) = browser.as_ref().map(|b| b.identifier()) else {
                return;
            };
            crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                crate::runtime::CefEvent::TitleChangedBrowser(
                    crate::browser::event::TitleChangedBrowserEvent {
                        browser_id,
                        title: title.map(CefString::to_string),
                    },
                ),
            ));
        }
    }
}

wrap_load_handler! {
    struct BrowserLoadHandler {}

    impl LoadHandler {
        fn on_load_error(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                crate::runtime::CefEvent::LoadErrorBrowserCallback(
                    crate::browser::event::LoadErrorBrowserCallbackEvent {
                        browser: browser.cloned(),
                        frame: frame.cloned(),
                        error_code,
                        error_text: error_text.map(CefString::to_string),
                        failed_url: failed_url.map(CefString::to_string),
                    },
                ),
            ));
        }

        fn on_loading_state_change(
            &self,
            browser: Option<&mut Browser>,
            is_loading: std::os::raw::c_int,
            _can_go_back: std::os::raw::c_int,
            _can_go_forward: std::os::raw::c_int,
        ) {
            let Some(browser_id) = browser.as_ref().map(|b| b.identifier()) else {
                return;
            };
            crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                crate::runtime::CefEvent::LoadingStateChangedBrowser(
                    crate::browser::event::LoadingStateChangedBrowserEvent {
                        browser_id,
                        is_loading,
                    },
                ),
            ));
        }
    }
}

wrap_task! {
    struct NavigateCefBrowser {
        browser_id: i32,
        go_forward: bool,
    }

    impl Task {
        fn execute(&self) {
            debug_assert!(crate::browser::cef::on_cef_ui_thread());
            crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                crate::runtime::CefEvent::NavigateBrowser(crate::browser::event::NavigateBrowserEvent {
                    browser_id: self.browser_id,
                    go_forward: self.go_forward,
                }),
            ));
        }
    }
}

wrap_task! {
    struct LinkHintsRun {
        browser_id: i32,
        show: bool,
    }

    impl Task {
        fn execute(&self) {
            debug_assert!(crate::browser::cef::on_cef_ui_thread());
            if self.show {
                crate::runtime::send_user_event(crate::runtime::UserEvent::Input(
                    crate::runtime::ShellInputEvent::LinkHintsShowBrowser(
                        crate::browser::event::LinkHintsShowBrowserEvent {
                            browser_id: self.browser_id,
                        },
                    ),
                ));
            } else {
                crate::runtime::send_user_event(crate::runtime::UserEvent::Input(
                    crate::runtime::ShellInputEvent::LinkHintsHideBrowser(
                        crate::browser::event::LinkHintsHideBrowserEvent {
                            browser_id: self.browser_id,
                        },
                    ),
                ));
            }
        }
    }
}

wrap_task! {
    struct LinkHintsFeedTask {
        browser_id: i32,
        ch: char,
        prior_typed_len: usize,
    }

    impl Task {
        fn execute(&self) {
            debug_assert!(crate::browser::cef::on_cef_ui_thread());
            crate::runtime::send_user_event(crate::runtime::UserEvent::Input(
                crate::runtime::ShellInputEvent::LinkHintsFeedKeyDeferredBrowser(
                    crate::browser::event::LinkHintsFeedKeyDeferredBrowserEvent {
                        browser_id: self.browser_id,
                        ch: self.ch,
                        prior_typed_len: self.prior_typed_len,
                    },
                ),
            ));
        }
    }
}

wrap_task! {
    pub(crate) struct DelayedNavigationRepaint {
        browser_id: i32,
    }

    impl Task {
        fn execute(&self) {
            debug_assert!(crate::browser::cef::on_cef_ui_thread());
            crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                crate::runtime::CefEvent::DelayedNavigationRepaintBrowser(
                    crate::browser::event::DelayedNavigationRepaintBrowserEvent {
                        browser_id: self.browser_id,
                    },
                ),
            ));
        }
    }
}

wrap_task! {
    struct ShowMainWindow {}

    impl Task {
        fn execute(&self) {
            debug_assert!(crate::browser::cef::on_cef_ui_thread());
            crate::runtime::send_user_event(crate::runtime::UserEvent::App(
                crate::runtime::AppEvent::ShowMainWindowBrowser(
                    crate::browser::event::ShowMainWindowBrowserEvent,
                ),
            ));
        }
    }
}

wrap_task! {
    struct CloseAllBrowsers {
        force_close: bool,
    }

    impl Task {
        fn execute(&self) {
            debug_assert!(crate::browser::cef::on_cef_ui_thread());
            crate::browser::cef::closeall::close_all_browsers_on_ui_thread(self.force_close);
        }
    }
}
