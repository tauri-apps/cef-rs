use bevy_ecs::prelude::Resource;

use crate::window::pane_id::PaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowNodeId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneNodeId(pub u64);

#[derive(Debug, Clone)]
pub struct PaneNode {
    pub id: PaneNodeId,
    pub pane: PaneId,
}

#[derive(Debug, Clone)]
pub struct WindowNode {
    pub id: WindowNodeId,
    pub panes: Vec<PaneNode>,
}

#[derive(Resource, Debug, Default, Clone)]
pub struct SessionState {
    pub active_session: Option<SessionId>,
    pub windows: Vec<WindowNode>,
}
