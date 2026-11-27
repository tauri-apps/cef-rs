use bevy_ecs::prelude::Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct DefaultSplitAxis(pub SplitAxis);

impl Default for DefaultSplitAxis {
    fn default() -> Self {
        Self(SplitAxis::Horizontal)
    }
}
