//! Opaque pane identity for window/session state ([`crate::window::focus::FocusedPane`], [`crate::window::session`] tree).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);
