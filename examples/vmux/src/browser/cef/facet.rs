//! Per-browser ECS components for shell hints and address chrome.

use std::collections::HashMap;

use bevy_ecs::prelude::{Component, Resource};

/// Per-browser editable / IME hint snapshot for shell keyboard dispatch (built from ECS each batch).
pub type EditableFocusSnapshot = HashMap<i32, Option<bool>>;

#[inline]
pub fn editable_focus_is_typing(hints: &EditableFocusSnapshot, browser_id: i32) -> bool {
    matches!(hints.get(&browser_id), Some(Some(true)))
}

/// Max DOM ancestor walk depth used by editable-focus probing.
#[derive(Resource, Debug, Clone, Copy)]
pub struct DomFocusAncestorWalkMax(pub usize);

/// Per-browser editable focus hint mirrored from CEF probe results.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct EditableFocusHint(pub Option<bool>);

/// Per-browser fixed link-hints label width cache.
#[derive(Component, Debug, Clone, Copy)]
pub struct LinkHintsLabelWidth(pub u8);

impl Default for LinkHintsLabelWidth {
    fn default() -> Self {
        Self(1)
    }
}

/// Last observed URL for address bar updates in the embedded view.
#[derive(Component, Debug, Clone, Default)]
pub struct LastAddressUrl(pub Option<String>);
