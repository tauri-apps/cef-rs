//! Non-send OSR host bag: CEF client, window store handles, queues.
//! Winit / pump integration lives in [`crate::browser::cef::stage`], [`crate::window::dispatch`],
//! [`crate::window::system`], and Bevy systems.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use cef::Browser;
use cef::Client;
use cef::ImplBrowser;
use cef::ImplBrowserHost;
use cef::PaintElementType;
use winit::window::WindowId;

use crate::browser::cef::osr::CefAttach;
use crate::browser::cef::queue::BrowserUiOpQueue;
use crate::input::system::EditableFocusQueues;
use crate::settings::KeySettings;

pub struct OsrHostState {
    pub(crate) client_holder: Rc<RefCell<Option<Client>>>,
    /// Keeps [`cef::RequestContext`] alive for each created browser (matches `examples/osr`).
    pub(crate) cef_request_contexts: Vec<cef::RequestContext>,
    pub(crate) cef_attach: CefAttach,
    pub(crate) key_settings: Arc<KeySettings>,
    /// Last wins; applied in [`crate::browser::cef::active::apply_pending_set_active_browser_system`].
    /// `Cell` so pending set can run under `windows_store` guards.
    pub(crate) pending_set_active_browser: Cell<Option<i32>>,
    /// Drained by Bevy `Update` systems — see [`crate::input::system::EditableFocusQueues`].
    pub(crate) editable_focus_queues: EditableFocusQueues,
    /// Drained by [`crate::browser::cef::queue::apply_browser_ui_ops_system`].
    pub(crate) browser_ui_ops: BrowserUiOpQueue,
}

impl OsrHostState {
    pub fn new(
        client_holder: Rc<RefCell<Option<Client>>>,
        cef_attach: CefAttach,
        key_settings: Arc<KeySettings>,
        editable_focus_queues: EditableFocusQueues,
        browser_ui_ops: BrowserUiOpQueue,
    ) -> Self {
        Self {
            client_holder,
            cef_request_contexts: Vec::new(),
            cef_attach,
            key_settings,
            pending_set_active_browser: Cell::new(None),
            editable_focus_queues,
            browser_ui_ops,
        }
    }

    pub(crate) fn request_set_active_browser(&self, browser_id: i32) {
        self.pending_set_active_browser.set(Some(browser_id));
    }

    pub(crate) fn take_pending_set_active_browser(&self) -> Option<i32> {
        self.pending_set_active_browser.replace(None)
    }

    /// Invalidate CEF view + `request_redraw` after input routing (e.g. vimium) changed page chrome.
    pub(crate) fn nudge_osr_view_after_input(&self, window_id: WindowId) {
        let Ok(windows) = self.cef_attach.windows_store.lock() else {
            return;
        };
        let Some(entry) = windows.get(&window_id) else {
            return;
        };
        if let Some(h) = entry.browser.host() {
            h.invalidate(PaintElementType::VIEW);
            #[cfg(feature = "accelerated_osr")]
            h.send_external_begin_frame();
        }
        entry.surface.window.request_redraw();
    }

    pub(crate) fn browser_for_window(&self, window_id: WindowId) -> Option<Browser> {
        self.cef_attach
            .windows_store
            .lock()
            .ok()
            .and_then(|w| w.get(&window_id).map(|e| e.browser.clone()))
    }

    /// Resolves CEF `browser_id` for a [`WindowId`], preferring ECS [`BrowserId`](crate::browser::cef::entity::BrowserId) when it matches the CEF `windows_store`.
    pub(crate) fn browser_id_for_window(
        &self,
        window_id: WindowId,
        ecs: Option<(Entity, i32)>,
    ) -> Option<i32> {
        let store_bid = self
            .cef_attach
            .windows_store
            .lock()
            .ok()
            .and_then(|w| w.get(&window_id).map(|e| e.browser.identifier()));
        match (ecs, store_bid) {
            (Some((_, eid)), Some(sid)) if eid == sid => Some(sid),
            (Some(_), Some(sid)) => Some(sid),
            (None, Some(sid)) => Some(sid),
            (Some((_, eid)), None) => Some(eid),
            (None, None) => None,
        }
    }
}
