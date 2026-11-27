//! Browser domain: CEF OSR integration, render composite, and Bevy event wiring.
//!
//! [`BrowserPlugin`] is the supported surface; `cef` is `pub` so the `vmux` binary can run startup
//! without widening the whole crate. See `.cursor/rules/vmux-bevy-module-entry.mdc`.

pub mod cef;
pub mod event;
pub(crate) mod system;

use std::sync::{Arc, Mutex};

use bevy_app::{App as BevyApp, Plugin, Update};
use bevy_ecs::schedule::IntoSystemConfigs;

use crate::browser::cef::active::{
    ActiveBrowserId, NavigateActiveBrowserEvent, apply_pending_set_active_browser_system,
    navigate_active_browser_system,
};
use crate::browser::cef::entity::{
    CefBrowserHandles, CefBrowserHandlesInner, apply_browser_despawn_events,
    apply_browser_spawn_events,
};
use crate::browser::cef::ffi::register_browser_runtime_for_ffi;
use crate::browser::cef::focus::{
    EditableFocusProbeRequest, EditableShortcutKeyReplayRequest,
    dispatch_editable_focus_probe_requests_system,
    dispatch_editable_shortcut_key_replay_requests_system,
};
use crate::browser::cef::lifecycle::{
    BrowserCloseGuardsInner, BrowserCloseGuardsResource, BrowserLifecycleInner,
    BrowserLifecycleResource, RequestCloseAllBrowsersEvent,
    apply_close_all_browsers_requests_system,
};
use crate::browser::cef::queue::BrowserUiOpQueue;

/// CEF + browser ECS events and deferred apply systems.
pub struct BrowserPlugin;

impl BrowserPlugin {
    pub const fn new() -> Self {
        Self
    }
}

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut BevyApp) {
        let cef_handles = Arc::new(Mutex::new(CefBrowserHandlesInner::default()));
        let lifecycle = Arc::new(Mutex::new(BrowserLifecycleInner::default()));
        let close_guards = Arc::new(Mutex::new(BrowserCloseGuardsInner::default()));
        register_browser_runtime_for_ffi(
            cef_handles.clone(),
            lifecycle.clone(),
            close_guards.clone(),
        );
        app.insert_resource(CefBrowserHandles(cef_handles))
            .insert_resource(BrowserLifecycleResource(lifecycle))
            .insert_resource(BrowserCloseGuardsResource(close_guards));

        app.add_plugins(crate::browser::cef::CefPlugin::new())
            .init_resource::<event::OsrAfterCreatedAttachQueue>()
            .add_event::<cef::entity::BrowserSpawnEvent>()
            .add_event::<cef::entity::BrowserDespawnEvent>()
            .init_resource::<cef::entity::BrowserEntities>()
            .add_event::<event::NavigateBrowserEvent>()
            .add_event::<event::ReloadBrowserEvent>()
            .add_event::<event::DelayedNavigationRepaintBrowserEvent>()
            .add_event::<event::ShowMainWindowBrowserEvent>()
            .add_event::<event::CloseAllBrowsersBrowserEvent>()
            .add_event::<event::LinkHintsShowBrowserEvent>()
            .add_event::<event::LinkHintsHideBrowserEvent>()
            .add_event::<event::LinkHintsFeedKeyDeferredBrowserEvent>()
            .add_event::<event::ArmWindowlessCloseBrowserEvent>()
            .add_event::<event::QuitCloseAllBrowsersBrowserEvent>()
            .add_event::<event::AddressChangedBrowserEvent>()
            .add_event::<event::TitleChangedBrowserEvent>()
            .add_event::<event::LoadingStateChangedBrowserEvent>()
            .add_event::<event::AfterCreatedBrowserCallbackEvent>()
            .add_event::<event::BeforeCloseBrowserCallbackEvent>()
            .add_event::<event::DoCloseBrowserCallbackEvent>()
            .add_event::<event::LoadErrorBrowserCallbackEvent>()
            .add_event::<event::LinkHintsNavInvalidateBrowser>()
            .add_event::<event::BrowserShellEffectBatchEvent>()
            .add_event::<NavigateActiveBrowserEvent>()
            .add_event::<RequestCloseAllBrowsersEvent>()
            .add_event::<EditableFocusProbeRequest>()
            .add_event::<EditableShortcutKeyReplayRequest>()
            .init_resource::<BrowserUiOpQueue>()
            .init_resource::<ActiveBrowserId>()
            .add_systems(
                Update,
                (apply_browser_spawn_events, apply_browser_despawn_events).chain(),
            )
            .add_systems(
                Update,
                system::fanout_browser_shell_effect_batch_events_system.after(
                    crate::tmux::system::process_tmux_wm_shell_keyboard_events_system,
                ),
            )
            .add_systems(
                Update,
                cef::renderer::vmux_osr_flush_redraw_queue_system
                    .after(crate::window::system::apply_osr_host_window_dispatches_system),
            )
            .add_systems(
                Update,
                cef::queue::apply_browser_ui_ops_system
                    .after(cef::renderer::vmux_osr_flush_redraw_queue_system),
            )
            .add_systems(
                Update,
                dispatch_editable_focus_probe_requests_system
                    .after(crate::input::system::process_editable_focus_probe_queue_system),
            )
            .add_systems(
                Update,
                dispatch_editable_shortcut_key_replay_requests_system
                    .after(crate::input::system::process_editable_shortcut_key_replay_queue_system),
            )
            .add_systems(
                Update,
                apply_pending_set_active_browser_system
                    .after(dispatch_editable_shortcut_key_replay_requests_system),
            )
            .add_systems(
                Update,
                navigate_active_browser_system.after(apply_pending_set_active_browser_system),
            )
            .add_systems(
                Update,
                apply_close_all_browsers_requests_system
                    .after(navigate_active_browser_system)
                    .after(crate::runtime::handle_app_exit_for_graceful_shutdown),
            )
            .add_systems(
                Update,
                (
                    (
                        system::apply_navigate_browser_events_system
                            .after(system::fanout_browser_shell_effect_batch_events_system),
                        system::apply_reload_browser_events_system
                            .after(system::fanout_browser_shell_effect_batch_events_system),
                        system::apply_delayed_navigation_repaint_on_ui_events_system,
                        system::apply_show_main_window_on_ui_events_system,
                        system::apply_close_all_browsers_on_ui_events_system
                            .after(cef::lifecycle::apply_close_all_browsers_requests_system),
                        system::apply_link_hints_show_browser_events_system
                            .after(system::fanout_browser_shell_effect_batch_events_system)
                            .after(crate::vimium::apply_shortcut_key_replay_events_system),
                        system::apply_link_hints_hide_browser_events_system
                            .after(crate::vimium::apply_shortcut_key_replay_events_system),
                    ),
                    (
                        system::apply_link_hints_feed_key_deferred_browser_events_system
                            .after(system::fanout_browser_shell_effect_batch_events_system),
                        system::apply_arm_windowless_close_browser_events_system
                            .after(system::fanout_browser_shell_effect_batch_events_system),
                        system::apply_quit_close_all_browsers_browser_events_system
                            .after(system::fanout_browser_shell_effect_batch_events_system)
                            .before(cef::lifecycle::apply_close_all_browsers_requests_system),
                        system::apply_address_changed_browser_events_system,
                        system::apply_title_changed_browser_events_system,
                        (
                            system::enqueue_after_created_osr_attach_system,
                            system::apply_osr_browser_attach_system,
                        )
                            .chain(),
                        system::apply_loading_state_changed_browser_events_system
                            .after(system::apply_osr_browser_attach_system),
                        system::apply_before_close_browser_callback_events_system,
                        system::apply_do_close_browser_callback_events_system,
                        system::apply_load_error_browser_callback_events_system,
                    ),
                ),
            );
    }
}
