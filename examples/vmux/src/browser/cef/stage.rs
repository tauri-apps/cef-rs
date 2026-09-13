//! Winit runner / CEF pump integration: startup window, pending browser creation, redraw nudges.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use cef::*;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

use crate::browser::cef as vmux_cef;
use crate::browser::cef::context::{VmuxRequestContextHandler, VmuxRequestContextHandlerBuilder};
use crate::browser::cef::osr::PendingCefWindow;
use crate::browser::cef::renderer::WindowSurface;
use crate::runtime::ShutdownFlag;
use crate::runtime::{RuntimeState, ffi_osr_index};
use crate::window::registry::{show_all_windows, track_window};

use crate::browser::cef::shell::OsrHostState;
use crate::browser::cef::titles;

pub(crate) const WINDOW_TITLE: &str = "vmux";
/// If orderly quit never clears OSR maps (stuck CEF close), still exit the runner so the process can
/// run `cef::shutdown()` — otherwise Cmd+Q spins forever with `close_all count=1` every press.
const FORCE_QUIT_AFTER: Duration = Duration::from_millis(1200);

impl OsrHostState {
    /// Initial URL for `browser_host_create_browser` plus optional follow-up navigation.
    ///
    /// `http`/`https` startup URLs load **directly** so session history does not retain a leading
    /// `about:blank` entry (Back would otherwise leave the user on a blank page).
    ///
    /// Empty config still uses `about:blank` first, then the default startup URL — same deferred
    /// path as before (`deferred_url_after_blank`).
    pub fn staged_initial_navigation_url(startup_url: &str) -> (String, Option<String>) {
        let t = startup_url.trim();
        if t.is_empty() {
            return (
                "about:blank".to_string(),
                Some(crate::settings::DEFAULT_STARTUP_URL.to_string()),
            );
        }
        if t.eq_ignore_ascii_case("about:blank") {
            return ("about:blank".to_string(), None);
        }
        if t.starts_with("http://") || t.starts_with("https://") {
            return (t.to_string(), None);
        }
        (t.to_string(), None)
    }

    pub(crate) fn apply_pending_titles(&mut self) {
        let Ok(mut windows) = self.cef_attach.windows_store.lock() else {
            return;
        };
        for (browser_id, title) in titles::drain_titles() {
            if let Some(wid) = crate::browser::cef::osr::window_id_for_browser(
                crate::runtime::ffi_osr_index().as_ref(),
                browser_id,
            ) {
                if let Some(entry) = windows.get_mut(&wid) {
                    entry.surface.window.set_title(&title);
                }
            }
        }
    }

    /// Create winit window + wgpu surface and queue async CEF browser for `url` / window title.
    pub(crate) fn spawn_cef_browser_window(
        &mut self,
        rt: &mut RuntimeState,
        event_loop: &ActiveEventLoop,
        url: &str,
        title: &str,
    ) {
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "spawn_cef_browser_window: create_window title={title:?}"
        );
        let window = match event_loop.create_window(
            WindowAttributes::default()
                .with_title(title.to_string())
                .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0)),
        ) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                bevy_log::error!(
                    target: "vmux",
                    pid = std::process::id(),
                    "FATAL: winit create_window failed: {e:?}"
                );
                eprintln!("vmux: create_window failed: {e:?}");
                std::process::exit(1);
            }
        };
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "spawn_cef_browser_window: create_window OK"
        );
        vmux_cef::init_browser_client_macos_after_first_window(
            self.client_holder.as_ref(),
            self.cef_attach.clone(),
            window.clone(),
        );
        if self.client_holder.borrow().is_none() {
            bevy_log::error!(
                target: "vmux",
                pid = std::process::id(),
                "FATAL: spawn_cef_browser_window: Client is None (bootstrap did not run)"
            );
            eprintln!("vmux: fix OSR bootstrap or check RUST_LOG=vmux=info for earlier errors.");
            std::process::exit(1);
        }
        track_window(&window);
        window.set_visible(true);
        window.set_minimized(false);
        let _ = window.set_outer_position(winit::dpi::PhysicalPosition::new(80i32, 80i32));
        window.request_user_attention(None);
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "proof: winit_window_mapped_visible_requested title={title:?} (native NSWindow should appear even if CEF dies next)"
        );
        crate::log::record_startup_milestone("proof_winit_window_visible_requested");
        vmux_cef::set_device_scale_factor(window.scale_factor() as f32);
        window.set_ime_allowed(false);

        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "spawn_cef_browser_window: WindowSurface::new (wgpu surface)"
        );
        let surface = match WindowSurface::new(&*crate::runtime::ffi_gpu(), window.clone()) {
            Ok(s) => s,
            Err(e) => {
                bevy_log::error!(
                    target: "vmux",
                    pid = std::process::id(),
                    "FATAL: WindowSurface (wgpu): {e}"
                );
                eprintln!("vmux: WindowSurface failed: {e}");
                std::process::exit(1);
            }
        };
        let logical = surface
            .window
            .inner_size()
            .to_logical::<f32>(surface.window.scale_factor());

        surface.window.request_redraw();
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "proof: wgpu_surface_ready_for_osr logical={}x{}",
            logical.width,
            logical.height
        );
        crate::log::record_startup_milestone("proof_wgpu_surface_ready_for_osr");

        let (initial_url, deferred_url) = Self::staged_initial_navigation_url(url);
        rt.pending_browser_hosts.push_back(PendingCefWindow {
            surface,
            logical,
            url: initial_url,
            deferred_url,
        });
        self.cef_attach
            .unpaired_cef_shells
            .fetch_add(1, Ordering::Release);
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "spawn_cef_browser_window: queued pending CEF browser"
        );
    }

    /// After async [`browser_host_create_browser`], poll
    /// [`browser_host_get_browser_by_identifier`] (no Rust `LifeSpanHandler::on_after_created`).
    pub(crate) fn macos_poll_cef_browser_attach(&self, rt: &mut RuntimeState) {
        if !rt.macos_poll_attach_after_create {
            return;
        }
        {
            let fifo = self
                .cef_attach
                .shell_fifo
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if fifo.is_empty() {
                return;
            }
        }
        for bid in 1..=32i32 {
            if browser_host_get_browser_by_identifier(bid).is_some() {
                rt.macos_poll_attach_after_create = false;
                crate::log::record_startup_milestone("macos_polled_after_created_dispatch");
                bevy_log::info!(
                    target: "vmux",
                    pid = std::process::id(),
                    "macos: poll discovered browser_id={bid} — synthetic AfterCreated (no Rust lifespan handler)"
                );
                crate::runtime::send_user_event(crate::runtime::UserEvent::Cef(
                    crate::runtime::CefEvent::AfterCreatedBrowserCallback(
                        crate::browser::event::AfterCreatedBrowserCallbackEvent { browser_id: bid },
                    ),
                ));
                return;
            }
        }
    }

    /// Call once per main-loop iteration **after** the leading CEF pump and **before** `pump_app_events`.
    ///
    /// **Pre-refactor parity / `examples/osr`:** async [`browser_host_create_browser`] everywhere; **macOS**
    /// uses no Rust `LifeSpanHandler` and attaches via [`Self::macos_poll_cef_browser_attach`] after pumps
    /// (`browser_host_create_browser_sync` traps inside Chromium from this stack on some macOS 26 + CEF 146 builds).
    pub(crate) fn finish_next_pending_browser_if_any(&mut self, rt: &mut RuntimeState) -> bool {
        if rt.pending_browser_hosts.is_empty() {
            return false;
        }
        {
            let fifo = self
                .cef_attach
                .shell_fifo
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !fifo.is_empty() {
                return false;
            }
        }

        let Some(pending) = rt.pending_browser_hosts.pop_front() else {
            return false;
        };

        let Some(mut client) = self.client_holder.borrow().as_ref().map(Client::clone) else {
            bevy_log::error!(
                target: "vmux",
                pid = std::process::id(),
                "FATAL: finish_pending: Client is None"
            );
            std::process::exit(1);
        };

        let url = CefString::from(pending.url.as_str());
        let pre_attach_sz = pending.logical;
        self.cef_attach
            .shell_fifo
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(pending);
        if let Ok(mut g) = ffi_osr_index().pre_attach_view_logical.lock() {
            *g = Some(pre_attach_sz);
        }

        let accelerated_osr = cfg!(feature = "accelerated_osr");
        let window_info = WindowInfo {
            windowless_rendering_enabled: true as _,
            shared_texture_enabled: accelerated_osr as _,
            external_begin_frame_enabled: accelerated_osr as _,
            ..Default::default()
        };

        let disk_profile = vmux_cef::vmux_cef_disk_profile_abs_path();
        let _ = std::fs::create_dir_all(&disk_profile);
        let req_ctx_settings = RequestContextSettings {
            cache_path: CefString::from(disk_profile.as_str()),
            persist_session_cookies: 1,
            ..Default::default()
        };
        let mut req_ctx_handler =
            VmuxRequestContextHandlerBuilder::build(VmuxRequestContextHandler {});
        let Some(mut request_context) =
            request_context_create_context(Some(&req_ctx_settings), Some(&mut req_ctx_handler))
        else {
            let _ = self
                .cef_attach
                .shell_fifo
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_back();
            bevy_log::error!(
                target: "vmux",
                pid = std::process::id(),
                "FATAL: request_context_create_context returned None"
            );
            std::process::exit(1);
        };
        let browser_settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };

        crate::log::record_startup_milestone("finish_pending_before_create_browser");

        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "finish_pending: browser_host_create_browser (async)"
        );
        let rc = browser_host_create_browser(
            Some(&window_info),
            Some(&mut client),
            Some(&url),
            Some(&browser_settings),
            None,
            Some(&mut request_context),
        );

        if rc == 0 {
            let _ = self
                .cef_attach
                .shell_fifo
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_back();
            if let Ok(mut g) = ffi_osr_index().pre_attach_view_logical.lock() {
                g.take();
            }
            bevy_log::error!(
                target: "vmux",
                pid = std::process::id(),
                "FATAL: browser_host_create_browser returned {rc} (failure)"
            );
            std::process::exit(1);
        }

        if rc != 1 {
            bevy_log::info!(
                target: "vmux",
                pid = std::process::id(),
                "finish_pending: browser_host_create_browser returned {rc} (continuing; expected 1 on some CEF builds)"
            );
        }

        rt.macos_poll_attach_after_create = true;

        self.cef_request_contexts.push(request_context);
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "finish_pending: browser create accepted (on_after_created next or completed)"
        );
        crate::log::record_startup_milestone("finish_pending_create_browser_accepted");
        true
    }

    pub(crate) fn handle_resumed(&mut self, rt: &mut RuntimeState, event_loop: &ActiveEventLoop) {
        if rt.started {
            return;
        }
        rt.started = true;
        let startup_url = self.key_settings.startup_url.clone();
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "winit ApplicationHandler::resumed (single OSR window → {})",
            startup_url.as_str()
        );
        crate::log::record_startup_milestone("winit_resumed_spawn_window");
        self.spawn_cef_browser_window(rt, event_loop, startup_url.as_str(), WINDOW_TITLE);
        // Match `examples/osr`: `browser_host_create_browser*` runs in `ApplicationHandler::resumed`
        // after wgpu + window (not on the following `run_winit` head before `pump_app_events`).
        if self.finish_next_pending_browser_if_any(rt) {
            rt.bump_cef_post_create_pumps(24);
        }
        bevy_log::info!(
            target: "vmux",
            pid = std::process::id(),
            "resumed: startup window ready"
        );
    }

    /// Call from the main pump after the CEF tick and after `pump_app_events`.
    pub(crate) fn handle_about_to_wait(&mut self, rt: &mut RuntimeState, shutdown: &ShutdownFlag) {
        self.apply_pending_titles();
        show_all_windows();
        crate::window::wm::apply_window_layout(self, rt);
        crate::window::wm::apply_wm_hud_to_all_shell_windows(self, rt);

        if rt.quit_requested {
            rt.quit_started_at.get_or_insert(Instant::now());

            // Orphan `windows_store` rows (no matching `CefBrowserHandles` entry) block shutdown:
            // `before_close` may have missed `WindowId` from the foreign index, so the map never
            // shrank while handles were already cleared.
            let handles_empty = crate::browser::cef::ffi::ffi_browser_cef_handles()
                .lock()
                .map(|g| g.is_empty())
                .unwrap_or(true);
            if handles_empty {
                if let Ok(mut ws) = self.cef_attach.windows_store.lock() {
                    let before = ws.len();
                    if before > 0 {
                        ws.retain(|_, entry| {
                            let bid = entry.browser.identifier();
                            crate::browser::cef::ffi::ffi_browser_cef_handles()
                                .lock()
                                .map(|g| g.get(bid).is_some())
                                .unwrap_or(false)
                        });
                        let removed = before.saturating_sub(ws.len());
                        if removed > 0 {
                            bevy_log::info!(
                                target: "vmux",
                                pid = std::process::id(),
                                "quit_requested: dropped {removed} orphan windows_store entr(y/ies) (handles empty)"
                            );
                        }
                    }
                }
            }

            let ws_empty = self
                .cef_attach
                .windows_store
                .lock()
                .map(|m| m.is_empty())
                .unwrap_or(false);
            let fifo_empty = self
                .cef_attach
                .shell_fifo
                .lock()
                .map(|q| q.is_empty())
                .unwrap_or(false);
            let pending_empty = rt.pending_browser_hosts.is_empty();
            let orderly = ws_empty && fifo_empty && pending_empty;
            let force_deadline = rt
                .quit_started_at
                .is_some_and(|t| t.elapsed() >= FORCE_QUIT_AFTER);

            if orderly {
                crate::log::record_runtime_event(
                    "handle_about_to_wait shutdown orderly ws_empty fifo_empty pending_empty",
                );
                shutdown.0.store(true, std::sync::atomic::Ordering::Release);
            } else if force_deadline {
                crate::log::record_runtime_event(
                    "handle_about_to_wait shutdown FORCE_QUIT_AFTER (stalled CEF/OSR quit)",
                );
                bevy_log::warn!(
                    target: "vmux",
                    pid = std::process::id(),
                    "quit: forcing runner shutdown after {:?} — CEF close did not idle OSR maps (ws_empty={ws_empty} fifo_empty={fifo_empty} pending_empty={pending_empty})",
                    FORCE_QUIT_AFTER
                );
                shutdown.0.store(true, std::sync::atomic::Ordering::Release);
            }
        }

        let Ok(windows) = self.cef_attach.windows_store.lock() else {
            return;
        };
        for entry in windows.values() {
            entry.surface.window.request_redraw();
        }
        drop(windows);
        for p in &rt.pending_browser_hosts {
            p.surface.window.request_redraw();
        }
    }
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut, Resource, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct StagedNavIn(String);

#[cfg(test)]
#[derive(Resource, Default)]
struct StagedNavOut {
    url: String,
    deferred: Option<String>,
}

#[cfg(test)]
fn staged_nav_populate_system(inp: Res<StagedNavIn>, mut out: ResMut<StagedNavOut>) {
    let (u, d) = OsrHostState::staged_initial_navigation_url(&inp.0);
    out.url = u;
    out.deferred = d;
}

#[cfg(test)]
#[test]
fn https_loads_directly_without_blank_history_staging_via_system() {
    let mut world = World::default();
    world.insert_resource(StagedNavIn("https://www.google.com".into()));
    world.insert_resource(StagedNavOut::default());
    world.run_system_once(staged_nav_populate_system).unwrap();
    let o = world.resource::<StagedNavOut>();
    assert_eq!(o.url, "https://www.google.com");
    assert!(o.deferred.is_none());
}

#[cfg(test)]
#[test]
fn http_loads_directly_via_system() {
    let mut world = World::default();
    world.insert_resource(StagedNavIn("http://example.com/".into()));
    world.insert_resource(StagedNavOut::default());
    world.run_system_once(staged_nav_populate_system).unwrap();
    let o = world.resource::<StagedNavOut>();
    assert_eq!(o.url, "http://example.com/");
    assert!(o.deferred.is_none());
}

#[cfg(test)]
#[test]
fn explicit_about_blank_no_deferred_via_system() {
    let mut world = World::default();
    world.insert_resource(StagedNavIn("about:blank".into()));
    world.insert_resource(StagedNavOut::default());
    world.run_system_once(staged_nav_populate_system).unwrap();
    let o = world.resource::<StagedNavOut>();
    assert_eq!(o.url, "about:blank");
    assert!(o.deferred.is_none());
}

#[test]
fn whitespace_falls_back_to_blank_plus_default_via_system() {
    let mut world = World::default();
    world.insert_resource(StagedNavIn("   ".into()));
    world.insert_resource(StagedNavOut::default());
    world.run_system_once(staged_nav_populate_system).unwrap();
    let o = world.resource::<StagedNavOut>();
    assert_eq!(o.url, "about:blank");
    assert_eq!(
        o.deferred.as_deref(),
        Some(crate::settings::DEFAULT_STARTUP_URL)
    );
}
