//! ECS systems (`pub fn *_system`) and CEF UI-thread helpers for the browser domain.
//!
//! Bevy [`Event`] types and attach queues live in [`crate::browser::event`] (see
//! `.cursor/rules/vmux-bevy-module-entry.mdc`).

mod link_hints;
mod navigation;
mod ui;

pub use link_hints::{
    apply_link_hints_feed_key_deferred_browser_events_system,
    apply_link_hints_hide_browser_events_system,
    apply_link_hints_show_browser_events_system,
};

use bevy_ecs::event::{EventReader, EventWriter};
use bevy_ecs::prelude::{Res, ResMut};
use cef::rc::Rc;
use cef::{
    CefString, ImplBrowser, ImplBrowserHost, ImplFrame, ImplTask, ImplView, ImplWindow, Task,
    ThreadId, WrapTask, base64_encode, browser_host_get_browser_by_identifier,
    browser_view_get_for_browser, currently_on, post_task, uriencode, wrap_task,
};
use std::sync::atomic::Ordering;
use winit::window::WindowId;

use crate::browser::cef::ForeignOsrIndexResource;
use crate::browser::cef::closeall::close_all_browsers_on_ui_thread;
use crate::browser::cef::ffi::{
    ffi_browser_cef_attach, ffi_browser_cef_handles, ffi_browser_close_guards,
    ffi_browser_lifecycle,
};
use crate::browser::cef::hintsfeed::link_hints_read_session_with_browser;
use crate::browser::cef::lifecycle::RequestCloseAllBrowsersEvent;
use crate::browser::cef::lookup::cef_browser_by_id;
use crate::browser::event::*;

use navigation::apply_history_navigation;
use ui::{
    navigation_repaint_pass_on_ui, repaint_full_geometry, request_window_redraw_for_browser,
    sync_title_from_visible_navigation,
};

/// Turns [`BrowserShellEffectBatchEvent`] (from window / tmux shell dispatch) into the existing
/// per-kind browser [`Event`]s.
pub fn fanout_browser_shell_effect_batch_events_system(
    mut batches: EventReader<BrowserShellEffectBatchEvent>,
    mut navigate_events: EventWriter<NavigateBrowserEvent>,
    mut reload_events: EventWriter<ReloadBrowserEvent>,
    mut link_hints_show_events: EventWriter<LinkHintsShowBrowserEvent>,
    mut link_hints_hide_events: EventWriter<LinkHintsHideBrowserEvent>,
    mut link_hints_feed_events: EventWriter<LinkHintsFeedKeyDeferredBrowserEvent>,
    mut arm_windowless_close_events: EventWriter<ArmWindowlessCloseBrowserEvent>,
    mut quit_close_events: EventWriter<QuitCloseAllBrowsersBrowserEvent>,
) {
    for batch in batches.read() {
        let out = &batch.0;
        for &ev in out.navigate.iter() {
            navigate_events.send(ev);
        }
        for &ev in out.reload.iter() {
            reload_events.send(ev);
        }
        for &ev in out.link_hints_show.iter() {
            link_hints_show_events.send(ev);
        }
        for &ev in out.link_hints_hide.iter() {
            link_hints_hide_events.send(ev);
        }
        for &ev in out.link_hints_feed_key_deferred.iter() {
            link_hints_feed_events.send(ev);
        }
        for &ev in out.arm_windowless_close.iter() {
            arm_windowless_close_events.send(ev);
        }
        for &ev in out.quit_close_all_browsers.iter() {
            quit_close_events.send(ev);
        }
    }
}

pub fn apply_navigate_browser_events_system(
    mut events: EventReader<NavigateBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) == 0 {
            let mut task = NavigateCefBrowserPerformOnUiTask::new(ev.browser_id, ev.go_forward);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "apply_navigate_browser_events_system: post_task to UI thread failed"
                );
            }
            continue;
        }
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Some(browser) = cef_browser_by_id(ev.browser_id) else {
            continue;
        };
        apply_history_navigation(&browser, ev.go_forward);
        if let Some(host) = browser.host() {
            host.set_focus(1);
        }
        let bid = browser.identifier();
        crate::browser::cef::osr::reset_paint_redraw_throttle(hub.0.as_ref(), bid);
        repaint_full_geometry(&browser);
        crate::browser::cef::message_loop_pump(10);
        crate::browser::cef::osr::reset_paint_redraw_throttle(hub.0.as_ref(), bid);
        repaint_full_geometry(&browser);
        crate::browser::cef::message_loop_pump(10);
        for _ in 0..3 {
            request_window_redraw_for_browser(hub.0.as_ref(), bid);
            crate::browser::cef::message_loop_pump(4);
        }
        navigation_repaint_pass_on_ui(hub.0.as_ref(), bid);
        sync_title_from_visible_navigation(&browser);
        request_window_redraw_for_browser(hub.0.as_ref(), bid);
        crate::browser::cef::message_loop_pump(3);
        let mut delayed = crate::browser::cef::client::DelayedNavigationRepaint::new(bid);
        let _ = cef::post_delayed_task(ThreadId::UI, Some(&mut delayed), 75);
    }
}

pub fn apply_reload_browser_events_system(
    mut events: EventReader<ReloadBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let attach = ffi_browser_cef_attach();
            let from_store = attach.as_ref().and_then(|a| {
                a.windows_store.lock().ok().and_then(|ws| {
                    ws.values()
                        .find(|e| e.browser.identifier() == ev.browser_id)
                        .map(|e| e.browser.clone())
                })
            });
            let browser = from_store.or_else(|| {
                ffi_browser_cef_handles()
                    .lock()
                    .ok()
                    .and_then(|g| g.get(ev.browser_id).cloned())
            });
            let Some(browser) = browser else {
                continue;
            };
            browser.reload();
            let bid = browser.identifier();
            crate::browser::cef::osr::reset_paint_redraw_throttle(hub.0.as_ref(), bid);
            crate::browser::cef::message_loop_pump(24);
            request_window_redraw_for_browser(hub.0.as_ref(), bid);
            continue;
        }
        let mut task = ReloadBrowserPerformOnUiTask::new(ev.browser_id);
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_reload_browser_events_system: post_task to UI thread failed"
            );
        }
    }
}

pub fn apply_delayed_navigation_repaint_on_ui_events_system(
    mut events: EventReader<DelayedNavigationRepaintBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let attach = ffi_browser_cef_attach();
            let from_store = attach.as_ref().and_then(|a| {
                a.windows_store.lock().ok().and_then(|ws| {
                    ws.values()
                        .find(|e| e.browser.identifier() == ev.browser_id)
                        .map(|e| e.browser.clone())
                })
            });
            let browser = from_store.or_else(|| {
                ffi_browser_cef_handles()
                    .lock()
                    .ok()
                    .and_then(|g| g.get(ev.browser_id).cloned())
            });
            let Some(browser) = browser else {
                continue;
            };
            let bid = ev.browser_id;
            crate::browser::cef::osr::reset_paint_redraw_throttle(hub.0.as_ref(), bid);
            if let Some(host) = browser.host() {
                host.was_resized();
                host.notify_screen_info_changed();
                host.invalidate(cef::PaintElementType::VIEW);
                #[cfg(feature = "accelerated_osr")]
                host.send_external_begin_frame();
            }
            crate::browser::cef::message_loop_pump(10);
            request_window_redraw_for_browser(hub.0.as_ref(), bid);
            continue;
        }
        let mut task = DelayedNavigationRepaintPerformOnUiTask::new(ev.browser_id);
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_delayed_navigation_repaint_on_ui_events_system: post_task failed"
            );
        }
    }
}

pub fn apply_show_main_window_on_ui_events_system(
    mut events: EventReader<ShowMainWindowBrowserEvent>,
) {
    for _ in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let handles = ffi_browser_cef_handles();
            let Ok(guard) = handles.lock() else {
                continue;
            };
            let Some(first_id) = guard.first_browser_id() else {
                continue;
            };
            let Some(mut main_browser) = guard.get(first_id).cloned() else {
                continue;
            };
            drop(guard);
            if let Some(browser_view) = browser_view_get_for_browser(Some(&mut main_browser)) {
                if let Some(window) = browser_view.window() {
                    window.show();
                }
            } else {
                crate::window::registry::show_all_windows();
            }
            continue;
        }
        let mut task = ShowMainWindowPerformOnUiTask::new();
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_show_main_window_on_ui_events_system: post_task failed"
            );
        }
    }
}

pub fn apply_close_all_browsers_on_ui_events_system(
    mut events: EventReader<CloseAllBrowsersBrowserEvent>,
) {
    for ev in events.read() {
        if currently_on(ThreadId::UI) != 0 {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            close_all_browsers_on_ui_thread(ev.force_close);
            continue;
        }
        let mut task = CloseAllBrowsersPerformOnUiTask::new(ev.force_close);
        if post_task(ThreadId::UI, Some(&mut task)) == 0 {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "apply_close_all_browsers_on_ui_events_system: post_task failed"
            );
        }
    }
}

pub fn apply_arm_windowless_close_browser_events_system(
    mut events: EventReader<ArmWindowlessCloseBrowserEvent>,
) {
    for _ in events.read() {
        if let Ok(mut g) = ffi_browser_close_guards().lock() {
            g.allow_next_windowless_do_close = true;
        }
    }
}

pub fn apply_quit_close_all_browsers_browser_events_system(
    mut events: EventReader<QuitCloseAllBrowsersBrowserEvent>,
    mut close_all: EventWriter<RequestCloseAllBrowsersEvent>,
) {
    for _ in events.read() {
        close_all.send(RequestCloseAllBrowsersEvent { force_close: true });
    }
}

pub fn apply_address_changed_browser_events_system(
    mut events: EventReader<AddressChangedBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
    mut link_hints_nav_invalidate: EventWriter<LinkHintsNavInvalidateBrowser>,
) {
    for ev in events.read() {
        let _ = ev.url.as_deref();
        if ffi_browser_cef_attach().is_none() {
            continue;
        }
        let Some(browser) = cef_browser_by_id(ev.browser_id) else {
            continue;
        };
        let bid = browser.identifier();
        let changed = true;
        sync_title_from_visible_navigation(&browser);
        if changed {
            if ffi_browser_cef_attach().is_some() {
                crate::browser::cef::osr::reset_paint_redraw_throttle(hub.0.as_ref(), bid);
                if let Some(host) = browser.host() {
                    host.was_resized();
                    host.notify_screen_info_changed();
                    host.invalidate(cef::PaintElementType::VIEW);
                    #[cfg(feature = "accelerated_osr")]
                    host.send_external_begin_frame();
                }
                request_window_redraw_for_browser(hub.0.as_ref(), bid);
                crate::browser::cef::message_loop_pump(4);
            }
            let overlay_likely_up = currently_on(ThreadId::UI) != 0
                && link_hints_read_session_with_browser(&browser).still_active;
            if !overlay_likely_up {
                link_hints_nav_invalidate.send(LinkHintsNavInvalidateBrowser { browser_id: bid });
            }
        } else {
            request_window_redraw_for_browser(hub.0.as_ref(), bid);
            #[cfg(feature = "accelerated_osr")]
            if let Some(host) = browser.host() {
                host.send_external_begin_frame();
            }
            crate::browser::cef::message_loop_pump(4);
        }
    }
}

pub fn apply_title_changed_browser_events_system(
    mut events: EventReader<TitleChangedBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read() {
        let browser_id = ev.browser_id;
        let title = ev.title.as_deref();
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let mut browser = cef_browser_by_id(browser_id);
        if let Some(browser_view) = browser_view_get_for_browser(browser.as_mut()) {
            if let Some(window) = browser_view.window() {
                let title_cef = title.map(CefString::from);
                window.set_title(title_cef.as_ref());
                continue;
            }
        }
        if let Some(t) = title {
            crate::browser::cef::titles::push_title(browser_id, t.to_string());
        }
        if ffi_browser_cef_attach().is_some() {
            if let Some(ref b) = browser {
                let bid = b.identifier();
                if let Some(host) = b.host() {
                    host.invalidate(cef::PaintElementType::default());
                    #[cfg(feature = "accelerated_osr")]
                    host.send_external_begin_frame();
                }
                request_window_redraw_for_browser(hub.0.as_ref(), bid);
                crate::browser::cef::message_loop_pump(4);
            }
        }
    }
}

pub fn apply_loading_state_changed_browser_events_system(
    mut events: EventReader<LoadingStateChangedBrowserEvent>,
    hub: Res<ForeignOsrIndexResource>,
    mut link_hints_nav_invalidate: EventWriter<LinkHintsNavInvalidateBrowser>,
) {
    for ev in events.read() {
        let browser_id = ev.browser_id;
        let Some(browser) = cef_browser_by_id(browser_id) else {
            continue;
        };
        if ffi_browser_cef_attach().is_none() {
            continue;
        }
        if ev.is_loading != 0 {
            link_hints_nav_invalidate.send(LinkHintsNavInvalidateBrowser { browser_id });
            if let Some(host) = browser.host() {
                host.invalidate(cef::PaintElementType::VIEW);
                #[cfg(feature = "accelerated_osr")]
                host.send_external_begin_frame();
            }
            request_window_redraw_for_browser(hub.0.as_ref(), browser_id);
            crate::browser::cef::message_loop_pump(5);
            continue;
        }
        // Staged startup: `about:blank` may finish before `deferred_url_after_blank` is inserted
        // (attach runs later or loses same-tick ordering). [`apply_osr_browser_attach_system`] also
        // posts this task after insert so the real `startup_url` always loads.
        if ffi_browser_cef_attach().is_some() {
            let mut task = ConsumeDeferredStartupNavTask::new(browser_id);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "apply_loading_state_changed: post_task failed for deferred startup URL — sync load_url"
                );
                let Some(attach) = ffi_browser_cef_attach() else {
                    continue;
                };
                let next_url = attach
                    .deferred_url_after_blank
                    .lock()
                    .ok()
                    .and_then(|mut m| m.remove(&browser_id));
                if let Some(next_url) = next_url {
                    let u = CefString::from(next_url.as_str());
                    if let Some(frame) = browser.main_frame() {
                        frame.load_url(Some(&u));
                    }
                    crate::browser::cef::message_loop_pump(8);
                }
            }
        }
        repaint_full_geometry(&browser);
        crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
            crate::runtime::CefEvent::SetEditableFocusHint {
                browser_id,
                editable: false,
            },
        ));
        request_window_redraw_for_browser(hub.0.as_ref(), browser_id);
        crate::browser::cef::focus::schedule_editable_focus_probe(browser_id);
    }
}

/// Handles [`AfterCreatedBrowserCallbackEvent`]: registers the browser in [`CefBrowserHandlesInner`],
/// pops the matching OSR shell from the fifo, and enqueues [`OsrAfterCreatedWork`] for
/// [`apply_osr_browser_attach_system`]. Windowless spawns finish here.
pub(crate) fn enqueue_after_created_osr_attach_system(
    mut events: EventReader<AfterCreatedBrowserCallbackEvent>,
    attach_queue: ResMut<OsrAfterCreatedAttachQueue>,
) {
    for ev in events.read() {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let browser_id = ev.browser_id;
        let Some(browser) = browser_host_get_browser_by_identifier(browser_id) else {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "AfterCreatedBrowserCallbackEvent: browser_host_get_browser_by_identifier({browser_id}) returned None (ignored)"
            );
            continue;
        };
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "AfterCreated: resolved browser_id={browser_id} via get_browser_by_identifier"
        );
        if let Some(ref attach) = ffi_browser_cef_attach() {
            if let Ok(mut g) = ffi_browser_cef_handles().lock() {
                g.insert_spawn(browser_id, browser.clone());
            }
            let shell = attach
                .shell_fifo
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front();
            let Some(shell) = shell else {
                bevy_log::error!(target: "vmux", pid = std::process::id(), "FATAL: on_after_created: no pending OSR shell (fifo empty)");
                std::process::exit(1);
            };
            attach_queue
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_back(OsrAfterCreatedWork {
                    browser_id,
                    browser,
                    shell,
                });
        } else {
            if let Ok(mut g) = ffi_browser_cef_handles().lock() {
                g.insert_spawn(browser_id, browser.clone());
            }
            crate::runtime::send_user_event(crate::runtime::UserEvent::App(
                crate::runtime::AppEvent::BrowserSpawn(crate::runtime::BrowserSpawnEvent {
                    browser_id,
                    window_id: WindowId::dummy(),
                    browser,
                    osr_view_logical_size: None,
                    osr_paint_bind_group: None,
                }),
            ));
        }
    }
}

/// Drains [`OsrAfterCreatedAttachQueue`]: [`crate::browser::cef::osr::register_tab`], `windows_store`, spawn + focus-hint user events.
pub(crate) fn apply_osr_browser_attach_system(
    attach_queue: ResMut<OsrAfterCreatedAttachQueue>,
    hub: Res<ForeignOsrIndexResource>,
    mut rt: ResMut<crate::runtime::RuntimeState>,
) {
    let Some(attach) = ffi_browser_cef_attach() else {
        return;
    };
    let mut q = attach_queue.0.lock().unwrap_or_else(|e| e.into_inner());
    while let Some(work) = q.pop_front() {
        let browser_id = work.browser_id;
        let browser = work.browser;
        let shell = work.shell;
        let wid = shell.surface.window.id();
        let shared =
            crate::browser::cef::osr::register_tab(hub.0.as_ref(), browser_id, wid, shell.logical);
        let browser_for_ecs = browser.clone();
        let mut ws = attach
            .windows_store
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        ws.insert(
            wid,
            crate::browser::cef::osr::WindowEntry {
                surface: shell.surface,
                browser,
                size: shared.view_logical_size.clone(),
            },
        );
        if let Some(entry) = ws.get(&wid) {
            entry.surface.window.request_redraw();
            let vis = entry.surface.window.is_visible();
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                "proof: osr_attached_browser_live browser_id={browser_id} winit_visible={vis:?} redraw_requested_for_swapchain"
            );
        }
        drop(ws);
        crate::tmux::layout::on_shell_window_attached(&mut rt, wid);
        if let Some(next_url) = shell.deferred_url {
            if let Ok(mut m) = attach.deferred_url_after_blank.lock() {
                m.insert(browser_id, next_url);
            }
            let mut task = ConsumeDeferredStartupNavTask::new(browser_id);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "apply_osr_browser_attach: post_task failed for deferred startup URL — sync load_url"
                );
                let next_url = attach
                    .deferred_url_after_blank
                    .lock()
                    .ok()
                    .and_then(|mut m| m.remove(&browser_id));
                if let Some(next_url) = next_url {
                    let u = CefString::from(next_url.as_str());
                    if let Some(browser) = cef_browser_by_id(browser_id) {
                        if let Some(frame) = browser.main_frame() {
                            frame.load_url(Some(&u));
                        }
                    }
                    crate::browser::cef::message_loop_pump(8);
                }
            }
        }
        crate::runtime::send_user_event(crate::runtime::UserEvent::App(
            crate::runtime::AppEvent::BrowserSpawn(crate::runtime::BrowserSpawnEvent {
                browser_id,
                window_id: wid,
                browser: browser_for_ecs,
                osr_view_logical_size: Some(shared.view_logical_size.clone()),
                osr_paint_bind_group: Some(shared.paint_bind_group.clone()),
            }),
        ));
        crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
            crate::runtime::CefEvent::SetEditableFocusHint {
                browser_id,
                editable: false,
            },
        ));
        attach.unpaired_cef_shells.fetch_sub(1, Ordering::Release);
    }
}

pub fn apply_before_close_browser_callback_events_system(
    mut events: EventReader<BeforeCloseBrowserCallbackEvent>,
    hub: Res<ForeignOsrIndexResource>,
) {
    for ev in events.read().cloned() {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Some(browser) = ev.browser else {
            bevy_log::warn!(
                target: "vmux",
                pid = std::process::id(),
                "BeforeCloseBrowserCallbackEvent: browser is None (ignored)"
            );
            continue;
        };
        let removed_id = browser.identifier();
        crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
            crate::runtime::CefEvent::RemoveBrowserEntries {
                browser_id: removed_id,
            },
        ));
        let wid_for_removed = if ffi_browser_cef_attach().is_some() {
            crate::browser::cef::osr::window_id_for_browser(hub.0.as_ref(), removed_id)
        } else {
            None
        };
        if let Ok(mut g) = ffi_browser_cef_handles().lock() {
            g.remove_despawn(removed_id);
        }
        crate::runtime::send_user_event(crate::runtime::UserEvent::App(
            crate::runtime::AppEvent::BrowserDespawn(crate::runtime::BrowserDespawnEvent {
                browser_id: removed_id,
            }),
        ));
        crate::browser::cef::osr::unregister_tab(hub.0.as_ref(), removed_id);

        if let Some(attach) = ffi_browser_cef_attach().as_ref() {
            let mut ws = attach
                .windows_store
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(wid) = wid_for_removed {
                ws.remove(&wid);
            }
            // If `browser_to_window` missed (race / ordering), `remove(wid)` never ran — then
            // `handle_about_to_wait` keeps seeing a non-empty `windows_store` and never sets
            // [`ShutdownFlag`], so Cmd+Q leaves the process spinning.
            let before_retain = ws.len();
            ws.retain(|_, entry| entry.browser.identifier() != removed_id);
            if wid_for_removed.is_none() && before_retain > 0 && before_retain == ws.len() {
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "before_close: windows_store still has no match for browser_id={removed_id} after foreign-index miss (stale map?)"
                );
            }
        }
        if ffi_browser_cef_handles()
            .lock()
            .map(|g| g.is_empty())
            .unwrap_or(true)
        {
            if let Some(ref attach) = ffi_browser_cef_attach() {
                let ws_empty = attach
                    .windows_store
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty();
                let fifo_empty = attach
                    .shell_fifo
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty();
                let unpaired = attach.unpaired_cef_shells.load(Ordering::Acquire);
                if !ws_empty || !fifo_empty || unpaired > 0 {
                    continue;
                }
            }
            if let Ok(mut g) = ffi_browser_close_guards().lock() {
                g.bypass_windowless_do_close_guard = false;
            }
            crate::log::record_runtime_event("before_close last browser: send ShutdownRequested");
            crate::runtime::send_user_event(crate::runtime::UserEvent::App(
                crate::runtime::AppEvent::ShutdownRequested,
            ));
        }
    }
}

pub fn apply_do_close_browser_callback_events_system(
    mut events: EventReader<DoCloseBrowserCallbackEvent>,
) {
    for _ in events.read() {
        let _ = do_close_from_event();
    }
}

pub fn apply_load_error_browser_callback_events_system(
    mut events: EventReader<LoadErrorBrowserCallbackEvent>,
) {
    for ev in events.read().cloned() {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let error_code = cef::sys::cef_errorcode_t::from(ev.error_code);
        if error_code == cef::sys::cef_errorcode_t::ERR_ABORTED {
            continue;
        }
        let error_code = error_code as i32;
        let Some(frame) = ev.frame.as_ref() else {
            continue;
        };
        let error_text = ev.error_text.as_deref().unwrap_or_default();
        let failed_url = ev.failed_url.as_deref().unwrap_or_default();
        let data = format!(
            r#"
            <html>
                <body bgcolor="white">
                    <h2>Failed to load URL {failed_url} with error {error_text} ({error_code}).</h2>
                </body>
            </html>
            "#
        );
        let data = CefString::from(&base64_encode(Some(data.as_bytes())));
        let uri = CefString::from(&uriencode(Some(&data), 0)).to_string();
        let uri = CefString::from(format!("data:text/html;base64,{uri}").as_str());
        let frame = frame.clone();
        frame.load_url(Some(&uri));
    }
}

wrap_task! {
    struct ReloadBrowserPerformOnUiTask {
        browser_id: i32,
    }

    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::Cef(
                    crate::runtime::CefEvent::ReloadBrowser(
                        ReloadBrowserEvent {
                            browser_id: self.browser_id,
                        },
                    ),
                ),
            );
        }
    }
}

wrap_task! {
    struct DelayedNavigationRepaintPerformOnUiTask {
        browser_id: i32,
    }
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::Cef(
                    crate::runtime::CefEvent::DelayedNavigationRepaintBrowser(
                        DelayedNavigationRepaintBrowserEvent {
                            browser_id: self.browser_id,
                        },
                    ),
                ),
            );
        }
    }
}

wrap_task! {
    struct ShowMainWindowPerformOnUiTask {}
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::App(
                    crate::runtime::AppEvent::ShowMainWindowBrowser(
                        ShowMainWindowBrowserEvent,
                    ),
                ),
            );
        }
    }
}

wrap_task! {
    struct CloseAllBrowsersPerformOnUiTask {
        force_close: bool,
    }
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            // Must call `close_browser` here on the CEF UI thread. Re-sending
            // `CloseAllBrowsersBrowserEvent` to the winit main thread ping-pongs forever when
            // `currently_on(UI)` is false during Bevy `Update` (external message pump).
            close_all_browsers_on_ui_thread(self.force_close);
        }
    }
}

wrap_task! {
    struct NavigateCefBrowserPerformOnUiTask {
        browser_id: i32,
        go_forward: bool,
    }
    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            crate::runtime::send_user_event(
                crate::runtime::UserEvent::Cef(
                    crate::runtime::CefEvent::NavigateBrowser(
                        NavigateBrowserEvent {
                            browser_id: self.browser_id,
                            go_forward: self.go_forward,
                        },
                    ),
                ),
            );
        }
    }
}

// Pop `deferred_url_after_blank` on the CEF UI thread and navigate (no-op if already consumed).
wrap_task! {
    struct ConsumeDeferredStartupNavTask {
        browser_id: i32,
    }

    impl Task {
        fn execute(&self) {
            debug_assert_ne!(currently_on(ThreadId::UI), 0);
            let Some(attach) = ffi_browser_cef_attach() else {
                return;
            };
            let next_url = attach
                .deferred_url_after_blank
                .lock()
                .ok()
                .and_then(|mut m| m.remove(&self.browser_id));
            let Some(next_url) = next_url else {
                return;
            };
            let Some(browser) = cef_browser_by_id(self.browser_id) else {
                return;
            };
            let u = CefString::from(next_url.as_str());
            if let Some(frame) = browser.main_frame() {
                frame.load_url(Some(&u));
            }
            crate::browser::cef::message_loop_pump(8);
        }
    }
}

pub fn do_close_from_event() -> i32 {
    debug_assert_ne!(currently_on(ThreadId::UI), 0);
    // Make closure system-driven: Bevy systems decide when to call `close_browser`,
    // and this synchronous CEF callback simply permits destruction.
    if ffi_browser_cef_handles()
        .lock()
        .map(|g| g.len() == 1)
        .unwrap_or(false)
    {
        if let Ok(mut g) = ffi_browser_lifecycle().lock() {
            g.is_closing = true;
        }
    }
    0
}
