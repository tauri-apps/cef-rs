//! Which browser is active for back/forward and related shortcuts.

use bevy_ecs::event::EventReader;
use bevy_ecs::prelude::{Event, EventWriter, NonSendMut, Res, ResMut, Resource};

use crate::browser::cef::entity::CefBrowserHandles;
use crate::browser::cef::shell::OsrHostState;
use crate::browser::event::NavigateBrowserEvent;

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveBrowserId(pub Option<i32>);

/// Back/forward request that resolves through [`ActiveBrowserId`].
#[derive(Event, Debug, Clone, Copy)]
pub struct NavigateActiveBrowserEvent {
    pub go_forward: bool,
}

/// Prefer explicit focus ([`ActiveBrowserId`]); else first browser in [`CefBrowserHandlesInner::order`]
/// (ECS does not store tab order; the mutex map does).
pub fn resolve_active_browser_id(
    active: &ActiveBrowserId,
    handles: &CefBrowserHandles,
) -> Option<i32> {
    active
        .0
        .or_else(|| handles.0.lock().ok().and_then(|g| g.first_browser_id()))
}

/// ECS navigation entrypoint for active-browser shortcuts.
pub fn navigate_active_browser_system(
    active: Res<ActiveBrowserId>,
    handles: Res<CefBrowserHandles>,
    mut events: EventReader<NavigateActiveBrowserEvent>,
    mut navigate: EventWriter<NavigateBrowserEvent>,
) {
    for ev in events.read() {
        let Some(browser_id) = resolve_active_browser_id(&active, &handles) else {
            continue;
        };
        navigate.send(NavigateBrowserEvent {
            browser_id,
            go_forward: ev.go_forward,
        });
    }
}

/// Writes [`ActiveBrowserId`] from OSR host input (focus, clicks, …).
pub fn apply_pending_set_active_browser_system(
    osr_host: NonSendMut<OsrHostState>,
    mut active: ResMut<ActiveBrowserId>,
) {
    let Some(browser_id) = osr_host.take_pending_set_active_browser() else {
        return;
    };
    active.0 = Some(browser_id);
}
