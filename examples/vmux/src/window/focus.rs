use bevy_ecs::prelude::Resource;

use crate::window::pane_id::PaneId;

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct FocusedPane(pub Option<PaneId>);
