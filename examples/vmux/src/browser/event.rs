//! Bevy [`Event`] types and small event-related resources for the browser domain.
//!
//! ECS systems that consume these events live in [`crate::browser::system`] (see
//! `.cursor/rules/vmux-bevy-module-entry.mdc`).

use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex;

use bevy_ecs::prelude::{Event, Resource};
use cef::{Browser, Errorcode, Frame, ImplBrowser};
use winit::event::KeyEvent;
use winit::window::WindowId;

#[derive(Event, Debug, Clone, Copy)]
pub struct NavigateBrowserEvent {
    pub browser_id: i32,
    pub go_forward: bool,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct ReloadBrowserEvent {
    pub browser_id: i32,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct DelayedNavigationRepaintBrowserEvent {
    pub browser_id: i32,
}

#[derive(Event, Debug, Clone, Copy, Default)]
pub struct ShowMainWindowBrowserEvent;

#[derive(Event, Debug, Clone, Copy)]
pub struct CloseAllBrowsersBrowserEvent {
    pub force_close: bool,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct LinkHintsShowBrowserEvent {
    pub browser_id: i32,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct LinkHintsHideBrowserEvent {
    pub browser_id: i32,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct LinkHintsFeedKeyDeferredBrowserEvent {
    pub browser_id: i32,
    pub ch: char,
    pub prior_typed_len: usize,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct ArmWindowlessCloseBrowserEvent;

#[derive(Event, Debug, Clone, Copy)]
pub struct QuitCloseAllBrowsersBrowserEvent;

#[derive(Event, Debug, Clone)]
pub struct AddressChangedBrowserEvent {
    pub browser_id: i32,
    pub url: Option<String>,
}

#[derive(Event, Debug, Clone)]
pub struct TitleChangedBrowserEvent {
    pub browser_id: i32,
    pub title: Option<String>,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct LoadingStateChangedBrowserEvent {
    pub browser_id: i32,
    pub is_loading: i32,
}

/// Document navigated while link hints may be armed; drained into [`crate::runtime::LinkHintsNavPending`].
#[derive(Event, Debug, Clone, Copy)]
pub struct LinkHintsNavInvalidateBrowser {
    pub browser_id: i32,
}

/// After an editable-focus DOM probe, replay one key through the shell shortcut path (Chrome-like
/// shortcuts; vimium maps vim bindings onto the same pipeline).
#[derive(Event, Debug, Clone)]
pub struct ShortcutKeyReplayEvent {
    pub window_id: WindowId,
    pub event: KeyEvent,
}

/// CEF UI-thread link-hint feed completion mirrored into Bevy for the focused shell window.
#[derive(Event, Debug, Clone)]
pub struct LinkHintFeedEvent {
    pub window_id: WindowId,
    pub browser_id: i32,
    pub ch: char,
    pub prior_typed_len: usize,
    pub still_active: bool,
    pub hint_label_width: u8,
}

/// Browser-side effects collected while handling one OSR shell winit batch (navigation, link hints, quit).
#[derive(Clone, Default)]
pub struct BrowserEventBatch {
    pub navigate: Vec<NavigateBrowserEvent>,
    pub reload: Vec<ReloadBrowserEvent>,
    pub link_hints_show: Vec<LinkHintsShowBrowserEvent>,
    pub link_hints_hide: Vec<LinkHintsHideBrowserEvent>,
    pub link_hints_feed_key_deferred: Vec<LinkHintsFeedKeyDeferredBrowserEvent>,
    pub arm_windowless_close: Vec<ArmWindowlessCloseBrowserEvent>,
    pub quit_close_all_browsers: Vec<QuitCloseAllBrowsersBrowserEvent>,
}

/// One batch of shell-originated browser [`Event`]s (from window/tmux dispatch). Consumed by
/// [`crate::browser::system::fanout_browser_shell_effect_batch_events_system`], which sends the
/// per-kind browser events so existing `apply_*_browser_events_system` handlers stay unchanged.
#[derive(Event)]
pub struct BrowserShellEffectBatchEvent(pub BrowserEventBatch);

/// Popped OSR shell + browser, queued between
/// [`crate::browser::system::enqueue_after_created_osr_attach_system`] and
/// [`crate::browser::system::apply_osr_browser_attach_system`].
/// (`PendingCefWindow` is not `Clone`, so we use a resource queue instead of a Bevy `Event`.)
pub struct OsrAfterCreatedWork {
    pub browser_id: i32,
    pub browser: Browser,
    pub shell: crate::browser::cef::osr::PendingCefWindow,
}

#[derive(Resource, Default)]
pub struct OsrAfterCreatedAttachQueue(pub Mutex<VecDeque<OsrAfterCreatedWork>>);

/// CEF [`LifeSpanHandler::on_after_created`] notifies winit with **`browser_id` only** — no
/// [`Browser::clone`] in the callback (macOS + CEF 146 stability). Bevy resolves the handle via
/// [`cef::browser_host_get_browser_by_identifier`].
#[derive(Event, Clone, Copy)]
pub struct AfterCreatedBrowserCallbackEvent {
    pub browser_id: i32,
}

#[derive(Event, Clone)]
pub struct BeforeCloseBrowserCallbackEvent {
    pub browser: Option<Browser>,
}

#[derive(Event, Debug, Clone, Copy, Default)]
pub struct DoCloseBrowserCallbackEvent;

#[derive(Event, Clone)]
pub struct LoadErrorBrowserCallbackEvent {
    pub browser: Option<Browser>,
    pub frame: Option<Frame>,
    pub error_code: Errorcode,
    pub error_text: Option<String>,
    pub failed_url: Option<String>,
}

impl fmt::Debug for AfterCreatedBrowserCallbackEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AfterCreatedBrowserCallbackEvent")
            .field("browser_id", &self.browser_id)
            .finish()
    }
}

impl fmt::Debug for BeforeCloseBrowserCallbackEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BeforeCloseBrowserCallbackEvent")
            .field("browser", &self.browser.as_ref().map(|b| b.identifier()))
            .finish()
    }
}

impl fmt::Debug for LoadErrorBrowserCallbackEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadErrorBrowserCallbackEvent")
            .field("browser", &self.browser.as_ref().map(|b| b.identifier()))
            .field("has_frame", &self.frame.is_some())
            .field(
                "error_code",
                &((cef::sys::cef_errorcode_t::from(self.error_code)) as i32),
            )
            .field("error_text", &self.error_text)
            .field("failed_url", &self.failed_url)
            .finish()
    }
}
