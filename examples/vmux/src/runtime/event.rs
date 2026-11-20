//! Winit user-event taxonomy and pump-related [`Resource`]s ([`UserEvent`], [`RuntimeState`], …).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use bevy_ecs::event::EventReader;
use bevy_ecs::prelude::{ResMut, Resource};
use cef::sys::cef_event_flags_t;
use winit::event::KeyEvent;
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

use crate::browser::cef::entity::{BrowserDespawnEvent, BrowserSpawnEvent};
use crate::browser::cef::osr::PendingCefWindow;
use crate::browser::event;
use crate::settings::WmGeometryLayout;

/// Browsers whose document navigated while link hints may be armed; drained at window-event dispatch.
#[derive(Resource, Default)]
pub struct LinkHintsNavPending(pub Vec<i32>);

pub fn ingest_link_hints_nav_invalidate_events_system(
    mut events: EventReader<event::LinkHintsNavInvalidateBrowser>,
    mut pending: ResMut<LinkHintsNavPending>,
) {
    for ev in events.read() {
        pending.0.push(ev.browser_id);
    }
}

/// Winit + CEF pump scratch state (modifiers, pending hosts, find-mode keys, …).
#[derive(Resource)]
pub struct RuntimeState {
    pub primary_mouse_down: bool,
    pub last_cursor_pos: (i32, i32),
    pub mods_winit: ModifiersState,
    pub mods: cef_event_flags_t,
    pub wheel_residual: (f64, f64),
    pub started: bool,
    pub quit_requested: bool,
    /// Set when [`Self::quit_requested`] becomes true; used to force [`ShutdownFlag`] if CEF/OSR
    /// teardown stalls (non-empty `windows_store` after `close_browser`).
    pub quit_started_at: Option<Instant>,
    pub cef_post_create_pumps_remaining: u8,
    pub macos_poll_attach_after_create: bool,
    pub pending_browser_hosts: VecDeque<PendingCefWindow>,
    pub pending_find_mode_keys: VecDeque<(WindowId, KeyEvent)>,
    /// [`WindowEvent::RedrawRequested`](winit::event::WindowEvent::RedrawRequested) targets drained by [`crate::browser::cef::renderer::flush_osr_redraw_for_window`] after OSR dispatches.
    pub vmux_osr_redraw_queue: VecDeque<WindowId>,
    /// Tile vs stack geometry; initial value synced from `settings.toml` at startup.
    pub wm_geometry_layout: WmGeometryLayout,
    /// Rotates dwindle first-split axis (manual keybindings).
    pub wm_dwindle_axis_phase: u32,
    /// Last shell window that received `WindowEvent::Focused(true)`.
    pub wm_focused_shell_window: Option<WindowId>,
    /// Last monitor work-area size (physical px) for `balance_on_resize` in `[window_manager]`.
    pub wm_last_work_w: u32,
    pub wm_last_work_h: u32,
    /// After `ctrl+b` (or configured `[window_manager.tmux].prefix`), next key before this instant is interpreted as a tmux-style WM command.
    pub wm_tmux_prefix_deadline: Option<Instant>,
    /// BSP pane tree for manual/tmux tiling; `None` until first layout or first split.
    pub wm_pane_tree: Option<crate::tmux::pane_tree::WmPaneTree>,
    /// Set before spawning a split child; consumed when the new [`WindowId`] attaches.
    pub wm_pending_split: Option<crate::tmux::pane_tree::WmPendingSplit>,
    /// Short status for the tmux-style leader (shown in window title via [`crate::window::wm`]).
    pub wm_hud_message: String,
    /// When to clear [`Self::wm_hud_message`].
    pub wm_hud_clear_deadline: Option<Instant>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            primary_mouse_down: false,
            last_cursor_pos: (0, 0),
            mods_winit: ModifiersState::default(),
            mods: cef_event_flags_t::EVENTFLAG_NONE,
            wheel_residual: (0.0, 0.0),
            started: false,
            quit_requested: false,
            quit_started_at: None,
            cef_post_create_pumps_remaining: 0,
            macos_poll_attach_after_create: false,
            pending_browser_hosts: VecDeque::new(),
            pending_find_mode_keys: VecDeque::new(),
            vmux_osr_redraw_queue: VecDeque::new(),
            wm_geometry_layout: WmGeometryLayout::Tile,
            wm_dwindle_axis_phase: 0,
            wm_focused_shell_window: None,
            wm_last_work_w: 0,
            wm_last_work_h: 0,
            wm_tmux_prefix_deadline: None,
            wm_pane_tree: None,
            wm_pending_split: None,
            wm_hud_message: String::new(),
            wm_hud_clear_deadline: None,
        }
    }
}

impl RuntimeState {
    /// After `OsrHostState::finish_next_pending_browser_if_any` returns true, extend the multi-frame settle budget.
    pub fn bump_cef_post_create_pumps(&mut self, extra: u8) {
        self.cef_post_create_pumps_remaining = self
            .cef_post_create_pumps_remaining
            .saturating_add(extra)
            .min(64);
    }

    /// Drain up to `cap_per_frame` toward [`Self::cef_post_create_pumps_remaining`]; used by the main loop.
    pub fn drain_cef_post_create_pumps(&mut self, cap_per_frame: u8) -> u32 {
        let take = self.cef_post_create_pumps_remaining.min(cap_per_frame);
        self.cef_post_create_pumps_remaining -= take;
        take as u32
    }
}

/// Top-level payload for winit's user-event slot: app orchestration, CEF bridge, or shell input
/// (deferred keys, link-hint UI-thread ops — transport only; ECS/plugins interpret).
#[derive(Debug, Clone)]
pub enum UserEvent {
    App(AppEvent),
    Cef(CefEvent),
    Input(ShellInputEvent),
}

/// Window-manager actions from keybindings (`[window_manager.tmux.keybindings]` in `settings.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmAction {
    ToggleLayout,
    FocusNext,
    FocusPrev,
    SplitSideBySide,
    SplitStacked,
    BalanceWindows,
    RotateSplit,
}

/// Shell / lifecycle / window orchestration (not CEF callback-shaped, not Vimium-only).
#[derive(Debug, Clone)]
pub enum AppEvent {
    LinkHintFeed(event::LinkHintFeedEvent),
    /// CEF browser attached to a winit shell — spawn ECS entity on the main thread.
    BrowserSpawn(BrowserSpawnEvent),
    /// CEF browser is closing — despawn ECS entity.
    BrowserDespawn(BrowserDespawnEvent),
    /// CEF [`BrowserProcessHandler::on_schedule_message_pump_work`]: wake the loop and merge a pump deadline.
    ScheduleCefPump {
        deadline: Instant,
    },
    /// AppKit terminate / user quit — becomes [`bevy_app::AppExit`] in the winit user-event handler.
    RequestQuit,
    /// Last browser closed — request runner teardown (mirrors shell idle shutdown).
    ShutdownRequested,
    ShowMainWindowBrowser(event::ShowMainWindowBrowserEvent),
    CloseAllBrowsersBrowser(event::CloseAllBrowsersBrowserEvent),
    ArmWindowlessCloseBrowser(event::ArmWindowlessCloseBrowserEvent),
    QuitCloseAllBrowsersBrowser(event::QuitCloseAllBrowsersBrowserEvent),
    /// Spawn another OSR shell + browser (same startup URL as settings).
    RequestNewBrowserWindow,
    /// Spawn a new shell split from the focused (or primary) pane — tmux `"` / `%` style.
    /// `vertical_bar`: `true` = left|right (`%`), `false` = top/bottom (`"`).
    RequestSplitPane {
        vertical_bar: bool,
    },
    /// Tiling / stack / focus — handled on the winit thread with [`OsrHostState`].
    WindowManager(WmAction),
}

/// CEF integration: handlers, navigation, and UI-thread mirrors to the shell queue.
#[derive(Debug, Clone)]
pub enum CefEvent {
    /// CEF/UI-thread hint update mirrored into ECS shell-op queue.
    SetEditableFocusHint {
        browser_id: i32,
        editable: bool,
    },
    /// CEF/UI-thread browser close mirrored into ECS shell-op queue.
    RemoveBrowserEntries {
        browser_id: i32,
    },
    NavigateBrowser(event::NavigateBrowserEvent),
    ReloadBrowser(event::ReloadBrowserEvent),
    DelayedNavigationRepaintBrowser(event::DelayedNavigationRepaintBrowserEvent),
    AddressChangedBrowser(event::AddressChangedBrowserEvent),
    TitleChangedBrowser(event::TitleChangedBrowserEvent),
    LoadingStateChangedBrowser(event::LoadingStateChangedBrowserEvent),
    AfterCreatedBrowserCallback(event::AfterCreatedBrowserCallbackEvent),
    BeforeCloseBrowserCallback(event::BeforeCloseBrowserCallbackEvent),
    DoCloseBrowserCallback(event::DoCloseBrowserCallbackEvent),
    LoadErrorBrowserCallback(event::LoadErrorBrowserCallbackEvent),
}

/// Shell input and link-hint operations posted across threads into Bevy `Events<T>` (transport only).
#[derive(Debug, Clone)]
pub enum ShellInputEvent {
    ShortcutKeyReplay(event::ShortcutKeyReplayEvent),
    LinkHintsShowBrowser(event::LinkHintsShowBrowserEvent),
    LinkHintsHideBrowser(event::LinkHintsHideBrowserEvent),
    LinkHintsFeedKeyDeferredBrowser(event::LinkHintsFeedKeyDeferredBrowserEvent),
}

/// Back-compat alias for the winit user-event payload (same as [`UserEvent`]).
pub type AppUserEvent = UserEvent;

/// When the winit runner may exit: last browser is gone and CEF teardown is safe.
/// Unix signals use [`SignalQuitFlag`] and `request_quit()` instead of flipping this flag directly.
#[derive(Resource, Clone)]
pub struct ShutdownFlag(pub Arc<AtomicBool>);

/// Set by `SIGTERM` / `SIGINT` via `signal_hook`; drained each frame into `request_quit()` so shutdown
/// follows the same path as AppKit Quit (close browsers, then set [`ShutdownFlag`]).
#[derive(Resource, Clone)]
pub struct SignalQuitFlag(pub Arc<AtomicBool>);

/// Next time the external CEF message pump should run (main thread only; updated from winit user events).
#[derive(Resource)]
pub struct CefPumpDeadline(pub Option<Instant>);

impl Default for CefPumpDeadline {
    fn default() -> Self {
        Self(None)
    }
}

impl CefPumpDeadline {
    pub fn merge(&mut self, deadline: Instant) {
        self.0 = Some(match self.0 {
            Some(existing) => existing.min(deadline),
            None => deadline,
        });
    }

    pub fn next_wait_timeout(&self, default_idle: Duration) -> Duration {
        let Some(deadline) = self.0 else {
            return default_idle;
        };
        deadline
            .saturating_duration_since(Instant::now())
            .min(default_idle)
    }

    /// Returns `true` if a scheduled pump was due and clears the deadline.
    pub fn take_if_due(&mut self) -> bool {
        let Some(deadline) = self.0 else {
            return false;
        };
        if deadline <= Instant::now() {
            self.0 = None;
            return true;
        }
        false
    }
}
