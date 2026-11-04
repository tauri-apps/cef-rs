//! Explicit state machine for vimium-style OSR input (browse, link hints, insert, find, visual).
//!
//! ```text
//!                    ┌─────────┐
//!         ┌─────────►│ Browse  │◄────────────────────────────┐
//!         │          └────┬────┘                             │
//!         │               │ f (page safe)                    │
//!         │               ▼                                │
//!         │          ┌────────────┐   esc / ttl / editable  │
//!         │          │ LinkHints  │─────────────────────────┤
//!         │          └─────┬──────┘   pick (JS)            │
//!         │                │                                │
//!         │   i,/,v        │                                │
//!         ▼                │                                │
//!    ┌────────┐            │                                │
//!    │ Insert │────────────┼────────────────────────────────┘
//!    └────────┘  esc       │
//!    ┌────────┐            │
//!    │  Find  │────────────┘
//!    └────────┘  esc / enter
//!    ┌────────┐
//!    │ Visual │────────────────────────────────────────────┘
//!    └────────┘  esc
//! ```
//!
//! `LinkHints` is mutually exclusive with `Insert` / `Find` / `Visual`. `find_committed` and
//! `scroll_g_pending` apply only while in `Browse` (and persist across transient modes).
//!
//! Call sites use methods like [`VimiumState::is_find`] instead of matching on [`VimiumInputMode`]:
//! Pattern matching on `mode` stays in this module so transitions and predicates stay in one
//! place.

use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use bevy_ecs::prelude::Resource;

/// How long a link-hint session stays armed after `f` (overlay may appear a frame later).
pub const LINK_HINT_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimiumInputMode {
    /// j/k scroll, `f` hints, `n`/`N` find repeat, etc.
    Browse,
    LinkHints {
        browser_id: i32,
        until: Instant,
        /// Prefix typed so far (mirrors JS after each successful feed); survives flaky DOM probes.
        typed: String,
    },
    Insert {
        browser_id: i32,
    },
    Find {
        browser_id: i32,
        query: String,
    },
    Visual {
        browser_id: i32,
    },
}

impl Default for VimiumInputMode {
    fn default() -> Self {
        VimiumInputMode::Browse
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimiumChromeCleanup {
    Find,
    Visual,
}

#[derive(Debug, Clone)]
pub struct VimiumStateSnapshot {
    pub mode: VimiumInputMode,
    pub find_committed: String,
    pub scroll_g_pending: Option<Instant>,
}

impl Default for VimiumStateSnapshot {
    fn default() -> Self {
        Self {
            mode: VimiumInputMode::Browse,
            find_committed: String::new(),
            scroll_g_pending: None,
        }
    }
}

/// Shell-side vimium / hint state for OSR windows.
#[derive(Debug, Clone)]
pub struct VimiumState {
    mode: VimiumInputMode,
    /// After find `Enter`; used for `n` / `N` in `Browse`.
    pub find_committed: String,
    /// First `g` of `gg` (scroll top), only meaningful in `Browse`.
    pub scroll_g_pending: Option<Instant>,
}

impl Default for VimiumState {
    fn default() -> Self {
        Self {
            mode: VimiumInputMode::Browse,
            find_committed: String::new(),
            scroll_g_pending: None,
        }
    }
}

impl VimiumState {
    pub fn snapshot(&self) -> VimiumStateSnapshot {
        VimiumStateSnapshot {
            mode: self.mode.clone(),
            find_committed: self.find_committed.clone(),
            scroll_g_pending: self.scroll_g_pending,
        }
    }

    #[inline]
    fn in_link_hints(&self) -> bool {
        matches!(&self.mode, VimiumInputMode::LinkHints { .. })
    }

    #[inline]
    fn in_find_any(&self) -> bool {
        matches!(&self.mode, VimiumInputMode::Find { .. })
    }

    #[inline]
    fn in_any_ux(&self) -> bool {
        matches!(
            &self.mode,
            VimiumInputMode::Insert { .. }
                | VimiumInputMode::Find { .. }
                | VimiumInputMode::Visual { .. }
        )
    }

    pub fn link_hints_active(&self) -> bool {
        self.in_link_hints()
    }

    pub fn link_hints_browser_id(&self) -> Option<i32> {
        match &self.mode {
            VimiumInputMode::LinkHints { browser_id, .. } => Some(*browser_id),
            _ => None,
        }
    }

    pub fn link_hints_typed_prefix(&self) -> &str {
        match &self.mode {
            VimiumInputMode::LinkHints { typed, .. } => typed.as_str(),
            _ => "",
        }
    }

    pub fn link_hints_expired(&self, now: Instant) -> bool {
        match &self.mode {
            VimiumInputMode::LinkHints { until, .. } => now > *until,
            _ => false,
        }
    }

    pub fn arm_link_hints(&mut self, browser_id: i32, now: Instant) {
        self.mode = VimiumInputMode::LinkHints {
            browser_id,
            until: now + LINK_HINT_TTL,
            typed: String::new(),
        };
        self.scroll_g_pending = None;
    }

    pub fn link_hints_push_typed_char(&mut self, ch: char) {
        if let VimiumInputMode::LinkHints { typed, .. } = &mut self.mode {
            typed.push(ch);
        }
    }

    pub fn clear_link_hints(&mut self) {
        if self.in_link_hints() {
            self.mode = VimiumInputMode::Browse;
        }
    }

    pub fn clear_link_hints_if_browser(&mut self, browser_id: i32) -> bool {
        if self.link_hints_browser_id() == Some(browser_id) {
            self.clear_link_hints();
            return true;
        }
        false
    }

    pub fn find_swallows_keyup(&self, browser_id: i32) -> bool {
        self.is_find(browser_id)
    }

    pub fn ux_browser_id(&self) -> Option<i32> {
        match &self.mode {
            VimiumInputMode::Insert { browser_id }
            | VimiumInputMode::Find { browser_id, .. }
            | VimiumInputMode::Visual { browser_id } => Some(*browser_id),
            _ => None,
        }
    }

    pub fn chrome_cleanup(&self) -> Option<VimiumChromeCleanup> {
        match &self.mode {
            VimiumInputMode::Find { .. } => Some(VimiumChromeCleanup::Find),
            VimiumInputMode::Visual { .. } => Some(VimiumChromeCleanup::Visual),
            _ => None,
        }
    }

    pub fn is_insert(&self, browser_id: i32) -> bool {
        match &self.mode {
            VimiumInputMode::Insert { browser_id: b } => *b == browser_id,
            _ => false,
        }
    }

    pub fn is_find(&self, browser_id: i32) -> bool {
        match &self.mode {
            VimiumInputMode::Find { browser_id: b, .. } => *b == browser_id,
            _ => false,
        }
    }

    pub fn is_visual(&self, browser_id: i32) -> bool {
        match &self.mode {
            VimiumInputMode::Visual { browser_id: b } => *b == browser_id,
            _ => false,
        }
    }

    pub fn enter_insert(&mut self, browser_id: i32) {
        self.mode = VimiumInputMode::Insert { browser_id };
        self.scroll_g_pending = None;
    }

    pub fn enter_find(&mut self, browser_id: i32) {
        self.mode = VimiumInputMode::Find {
            browser_id,
            query: String::new(),
        };
        self.scroll_g_pending = None;
    }

    pub fn enter_visual(&mut self, browser_id: i32) {
        self.mode = VimiumInputMode::Visual { browser_id };
        self.scroll_g_pending = None;
    }

    pub fn exit_ux_to_browse(&mut self) {
        if self.in_any_ux() {
            self.mode = VimiumInputMode::Browse;
        }
    }

    pub fn find_query_mut(&mut self) -> Option<&mut String> {
        match &mut self.mode {
            VimiumInputMode::Find { query, .. } => Some(query),
            _ => None,
        }
    }

    pub fn expire_link_hints_if_due(&mut self, now: Instant) -> Option<i32> {
        if !self.link_hints_expired(now) {
            return None;
        }
        let bid = self.link_hints_browser_id()?;
        self.clear_link_hints();
        Some(bid)
    }

    pub fn finish_find_accept(&mut self) {
        if let VimiumInputMode::Find { query, .. } =
            std::mem::replace(&mut self.mode, VimiumInputMode::Browse)
        {
            self.find_committed = query;
        }
    }

    pub fn cancel_find(&mut self) {
        if self.in_find_any() {
            self.mode = VimiumInputMode::Browse;
            self.find_committed.clear();
        }
    }
}

/// Live vimium mode machine — separate from [`crate::runtime::RuntimeState`] (modifiers, pending shells, …).
#[derive(Resource)]
pub struct VimiumStateResource(pub VimiumState);

impl Default for VimiumStateResource {
    fn default() -> Self {
        Self(VimiumState::default())
    }
}

pub struct VimiumSession {
    vimium: VimiumState,
}

impl VimiumSession {
    pub fn new(vimium: VimiumState) -> Self {
        Self { vimium }
    }
}

impl Deref for VimiumSession {
    type Target = VimiumState;

    fn deref(&self) -> &Self::Target {
        &self.vimium
    }
}

impl DerefMut for VimiumSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.vimium
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct VimiumRuntimeResource {
    pub snapshot: VimiumStateSnapshot,
}
