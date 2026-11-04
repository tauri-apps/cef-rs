//! Winit [`ApplicationHandler`] + [`run_winit`] pump loop.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bevy_app::{App, AppExit};
use bevy_ecs::event::Events;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::WindowId;

use crate::browser::cef::entity::{BrowserDespawnEvent, BrowserSpawnEvent, CefBrowserHandles};
use crate::browser::cef::lifecycle::RequestCloseAllBrowsersEvent;
use crate::browser::cef::queue::{BrowserUiOp, BrowserUiOpQueue, enqueue_browser_ui_op};
use crate::browser::event;
use crate::window::system::PendingWindowEvents;

use super::event::{AppEvent, CefEvent, CefPumpDeadline, ShellInputEvent, ShutdownFlag, UserEvent};
use super::systems::{AppExitRequested, with_osr_host_runtime};
use crate::settings::{WmMode, WmTileStrategy};

fn push_shutdown_event(app: &mut App) {
    if let Some(mut ev) = app.world_mut().get_resource_mut::<Events<AppExit>>() {
        ev.send(AppExit::Success);
    }
}

/// AppKit `terminate:` / `request_quit` run outside normal ECS scheduling; mirror
/// [`super::systems::handle_app_exit_for_graceful_shutdown`] here so browser teardown starts in the
/// same turn (do not rely only on `AppExit` + `Update` ordering).
///
/// Do not bail out when [`AppExitRequested`] is already true: a prior quit may have failed to close
/// CEF browsers; retries must still emit [`RequestCloseAllBrowsersEvent`].
fn apply_immediate_graceful_teardown(app: &mut App) {
    crate::browser::cef::quit::try_begin_quit_visual_feedback();
    let has_browsers = app
        .world()
        .get_resource::<CefBrowserHandles>()
        .map(|h| h.0.lock().map(|g| !g.is_empty()).unwrap_or(false))
        .unwrap_or(false);

    let mut branch = "noop";
    if has_browsers {
        if let Some(mut ev) = app
            .world_mut()
            .get_resource_mut::<Events<RequestCloseAllBrowsersEvent>>()
        {
            ev.send(RequestCloseAllBrowsersEvent { force_close: true });
            branch = "sent_request_close_all_force";
        } else {
            branch = "has_browsers_missing_request_close_events";
        }
    } else if let Some(flag) = app.world_mut().get_resource_mut::<ShutdownFlag>() {
        flag.0.store(true, Ordering::Release);
        branch = "set_shutdown_flag";
    }

    let ar = if let Some(mut requested) = app.world_mut().get_resource_mut::<AppExitRequested>() {
        if !requested.0 {
            requested.0 = true;
            "app_exit_requested_set"
        } else {
            "app_exit_requested_already"
        }
    } else {
        "no_app_exit_requested_resource"
    };

    crate::log::record_runtime_event(&format!(
        "apply_immediate_graceful_teardown has_browsers={has_browsers} branch={branch} {ar}"
    ));

    if let Some(mut rt) = app
        .world_mut()
        .get_resource_mut::<super::event::RuntimeState>()
    {
        rt.quit_requested = true;
        rt.quit_started_at.get_or_insert(std::time::Instant::now());
    }
}

/// Winit [`ApplicationHandler`] state: staging for [`PendingWindowEvents`] and about-to-wait.
/// Logic lives in [`flush_winit_runner_pending_callbacks`] and [`winit_runner_user_event`] helpers, not on inherent methods.
pub struct WinitAppRunnerState<'a> {
    pub app: &'a mut App,
    pending_window_events: Vec<(WindowId, WindowEvent)>,
    pending_about_to_wait: bool,
}

/// After each `pump_app_events`, forward staged window events into the ECS queue and run shell idle work.
pub fn flush_winit_runner_pending_callbacks(runner: &mut WinitAppRunnerState<'_>) {
    if !runner.pending_window_events.is_empty() {
        let q = runner.app.world_mut().resource_mut::<PendingWindowEvents>();
        if let Ok(mut deque) = q.0.lock() {
            deque.extend(runner.pending_window_events.drain(..));
        }
    }
    if runner.pending_about_to_wait {
        runner.pending_about_to_wait = false;
        let shutdown = runner.app.world().resource::<ShutdownFlag>().clone();
        with_osr_host_runtime(runner.app.world_mut(), |osr_host, rt| {
            osr_host.handle_about_to_wait(rt, &shutdown);
        });
    }
}

fn spawn_split_browser_window(
    runner: &mut WinitAppRunnerState<'_>,
    event_loop: &ActiveEventLoop,
    vertical_bar: bool,
) {
    with_osr_host_runtime(runner.app.world_mut(), |osr_host, rt| {
        let ks = &*osr_host.key_settings;
        if ks.wm_mode == WmMode::Manual {
            rt.wm_dwindle_axis_phase = if vertical_bar { 0 } else { 1 };
        }
        if ks.effective_tile_strategy() == WmTileStrategy::Bsp {
            if let Some(anchor) = crate::tmux::layout::split_anchor_window_id(osr_host, rt) {
                rt.wm_pending_split = Some(crate::tmux::pane_tree::WmPendingSplit {
                    anchor,
                    vertical_bar,
                });
            }
        }
        bevy_log::info!(
            target: "vmux_wm",
            pid = std::process::id(),
            vertical_bar,
            tile_strategy = ?ks.effective_tile_strategy(),
            wm_pending_split = rt.wm_pending_split.is_some(),
            "spawn_split_browser_window: queued OSR shell (pending_split only for BSP)"
        );
        let url = osr_host.key_settings.startup_url.clone();
        osr_host.spawn_cef_browser_window(
            rt,
            event_loop,
            url.as_str(),
            crate::browser::cef::stage::WINDOW_TITLE,
        );
        if osr_host.finish_next_pending_browser_if_any(rt) {
            rt.bump_cef_post_create_pumps(24);
        }
    });
}

fn winit_runner_user_event(
    runner: &mut WinitAppRunnerState<'_>,
    event_loop: &ActiveEventLoop,
    event: UserEvent,
) {
    match event {
        UserEvent::App(app) => match app {
            AppEvent::LinkHintFeed(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::LinkHintFeedEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::BrowserSpawn(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<BrowserSpawnEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::BrowserDespawn(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<BrowserDespawnEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::ScheduleCefPump { deadline } => {
                if let Some(mut d) = runner.app.world_mut().get_resource_mut::<CefPumpDeadline>() {
                    d.merge(deadline);
                }
            }
            AppEvent::RequestQuit => {
                crate::log::record_runtime_event("winit_user_event RequestQuit");
                push_shutdown_event(runner.app);
                apply_immediate_graceful_teardown(runner.app);
            }
            AppEvent::ShutdownRequested => {
                if let Some(flag) = runner.app.world_mut().get_resource_mut::<ShutdownFlag>() {
                    flag.0.store(true, Ordering::Release);
                }
            }
            AppEvent::ShowMainWindowBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::ShowMainWindowBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::CloseAllBrowsersBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::CloseAllBrowsersBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::ArmWindowlessCloseBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::ArmWindowlessCloseBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::QuitCloseAllBrowsersBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::QuitCloseAllBrowsersBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            AppEvent::RequestNewBrowserWindow => {
                with_osr_host_runtime(runner.app.world_mut(), |osr_host, rt| {
                    rt.wm_pending_split = None;
                    let url = osr_host.key_settings.startup_url.clone();
                    osr_host.spawn_cef_browser_window(
                        rt,
                        event_loop,
                        url.as_str(),
                        crate::browser::cef::stage::WINDOW_TITLE,
                    );
                    if osr_host.finish_next_pending_browser_if_any(rt) {
                        rt.bump_cef_post_create_pumps(24);
                    }
                });
            }
            AppEvent::RequestSplitPane { vertical_bar } => {
                spawn_split_browser_window(runner, event_loop, vertical_bar);
            }
            AppEvent::WindowManager(action) => match action {
                super::event::WmAction::SplitSideBySide => {
                    spawn_split_browser_window(runner, event_loop, true);
                }
                super::event::WmAction::SplitStacked => {
                    spawn_split_browser_window(runner, event_loop, false);
                }
                other => {
                    with_osr_host_runtime(runner.app.world_mut(), |osr_host, rt| {
                        crate::window::wm::handle_wm_action(osr_host, rt, other);
                    });
                }
            },
        },
        UserEvent::Cef(cef) => match cef {
            CefEvent::SetEditableFocusHint {
                browser_id,
                editable,
            } => {
                let queue = runner.app.world().resource::<BrowserUiOpQueue>().clone();
                enqueue_browser_ui_op(
                    &queue,
                    BrowserUiOp::SetEditableFocusHint {
                        browser_id,
                        editable,
                    },
                );
            }
            CefEvent::RemoveBrowserEntries { browser_id } => {
                let queue = runner.app.world().resource::<BrowserUiOpQueue>().clone();
                enqueue_browser_ui_op(&queue, BrowserUiOp::RemoveBrowserEntries { browser_id });
            }
            CefEvent::NavigateBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::NavigateBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::ReloadBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::ReloadBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::DelayedNavigationRepaintBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::DelayedNavigationRepaintBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::AddressChangedBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::AddressChangedBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::TitleChangedBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::TitleChangedBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::LoadingStateChangedBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::LoadingStateChangedBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::AfterCreatedBrowserCallback(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::AfterCreatedBrowserCallbackEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::BeforeCloseBrowserCallback(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::BeforeCloseBrowserCallbackEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::DoCloseBrowserCallback(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::DoCloseBrowserCallbackEvent>>()
                {
                    events.send(event);
                }
            }
            CefEvent::LoadErrorBrowserCallback(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::LoadErrorBrowserCallbackEvent>>()
                {
                    events.send(event);
                }
            }
        },
        UserEvent::Input(v) => match v {
            ShellInputEvent::ShortcutKeyReplay(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::ShortcutKeyReplayEvent>>()
                {
                    events.send(event);
                }
            }
            ShellInputEvent::LinkHintsShowBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::LinkHintsShowBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            ShellInputEvent::LinkHintsHideBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::LinkHintsHideBrowserEvent>>()
                {
                    events.send(event);
                }
            }
            ShellInputEvent::LinkHintsFeedKeyDeferredBrowser(event) => {
                if let Some(mut events) = runner
                    .app
                    .world_mut()
                    .get_resource_mut::<Events<event::LinkHintsFeedKeyDeferredBrowserEvent>>()
                {
                    events.send(event);
                }
            }
        },
    }
}

fn winit_runner_resumed(runner: &mut WinitAppRunnerState<'_>, event_loop: &ActiveEventLoop) {
    with_osr_host_runtime(runner.app.world_mut(), |osr_host, rt| {
        osr_host.handle_resumed(rt, event_loop);
    });
    use crate::browser::cef::GpuResource;
    if runner.app.world().get_resource::<GpuResource>().is_none() {
        if let Some(gpu) = super::try_ffi_gpu() {
            runner.app.world_mut().insert_resource(GpuResource(gpu));
        }
    }
}

fn winit_runner_window_event(
    runner: &mut WinitAppRunnerState<'_>,
    event_loop: &ActiveEventLoop,
    window_id: WindowId,
    event: WindowEvent,
) {
    let _ = event_loop;
    runner.pending_window_events.push((window_id, event));
}

fn winit_runner_about_to_wait(runner: &mut WinitAppRunnerState<'_>, event_loop: &ActiveEventLoop) {
    let _ = event_loop;
    runner.pending_about_to_wait = true;
}

impl ApplicationHandler<UserEvent> for WinitAppRunnerState<'_> {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        winit_runner_user_event(self, event_loop, event);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        winit_runner_resumed(self, event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        winit_runner_window_event(self, event_loop, window_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        winit_runner_about_to_wait(self, event_loop);
    }
}

/// Wraps the macOS winit [`EventLoop`]. Browser `main` inserts this with
/// [`bevy_app::App::insert_non_send_resource`] before [`crate::VmuxPlugin`]; [`crate::runtime::RuntimePlugin`]
/// installs a runner that removes it and calls [`run_winit`].
pub struct VmuxMainEventLoop(pub EventLoop<UserEvent>);

pub fn build_event_loop() -> EventLoop<UserEvent> {
    let event_loop = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        EventLoop::<UserEvent>::with_user_event()
            .with_activation_policy(ActivationPolicy::Regular)
            .with_default_menu(false)
            .build()
            .expect("vmux: EventLoop::build")
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
}

pub fn run_winit(
    event_loop: &mut winit::event_loop::EventLoop<UserEvent>,
    mut app: App,
) -> AppExit {
    // `pump_app_events` wait when the loop is idle; do not add a second macOS sleep — a ~60ms sleep
    // per iteration capped input to ~17 Hz and caused dropped characters when typing fast.
    const IDLE_WAIT: Duration = Duration::from_millis(8);
    // CEF + winit + Bevy: ffi bridge + pump ordering stay in this module (not ECS systems)
    // until OSR is stable; see `OsrHostState::finish_next_pending_browser_if_any` for browser create.
    // Ensure Startup schedule runs before systems/resources that expect OsrHostState.
    app.update();
    crate::log::record_startup_milestone("run_winit_after_first_bevy_update");

    loop {
        if super::ffi_callbacks::take_pending_request_quit() {
            crate::log::record_runtime_event("run_winit drained pending_request_quit");
            push_shutdown_event(&mut app);
            apply_immediate_graceful_teardown(&mut app);
        }
        // Match `examples/osr`: Chromium `do_message_loop_work` before winit `pump_app_events`.
        crate::browser::cef::message_loop_pump(1);

        {
            with_osr_host_runtime(app.world_mut(), |osr_host, rt| {
                let issued_create = osr_host.finish_next_pending_browser_if_any(rt);
                if issued_create {
                    rt.bump_cef_post_create_pumps(24);
                }
                osr_host.macos_poll_cef_browser_attach(rt);
            });
        }

        let wait = app
            .world()
            .resource::<CefPumpDeadline>()
            .next_wait_timeout(IDLE_WAIT);
        let (status, mut runner_state) = {
            let mut runner_state = WinitAppRunnerState {
                app: &mut app,
                pending_window_events: Vec::new(),
                pending_about_to_wait: false,
            };
            let status = event_loop.pump_app_events(Some(wait), &mut runner_state);
            (status, runner_state)
        };
        flush_winit_runner_pending_callbacks(&mut runner_state);

        if let PumpStatus::Exit(_code) = status {
            crate::log::record_runtime_event("run_winit PumpStatus::Exit");
            push_shutdown_event(&mut app);
            apply_immediate_graceful_teardown(&mut app);
        }

        app.update();
        let shutdown_complete = app
            .world()
            .get_resource::<ShutdownFlag>()
            .map(|r| r.0.load(Ordering::Acquire))
            .unwrap_or(false);
        if shutdown_complete {
            break;
        }
        let post_create_chunk = with_osr_host_runtime(app.world_mut(), |_osr_host, rt| {
            rt.drain_cef_post_create_pumps(8)
        });
        let _ = app
            .world_mut()
            .resource_mut::<CefPumpDeadline>()
            .take_if_due();
        crate::browser::cef::main_message_loop_tick(post_create_chunk);
        with_osr_host_runtime(app.world_mut(), |osr_host, rt| {
            osr_host.macos_poll_cef_browser_attach(rt);
        });
    }

    // Keep CEF teardown coupled to the custom runner lifecycle.
    cef::quit_message_loop();
    cef::shutdown();

    AppExit::Success
}
