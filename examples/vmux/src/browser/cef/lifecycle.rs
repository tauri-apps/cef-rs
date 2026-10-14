use std::sync::{Arc, Mutex};

use bevy_ecs::event::{EventReader, EventWriter};
use bevy_ecs::prelude::{Event, Resource};

#[derive(Default)]
pub struct BrowserLifecycleInner {
    pub is_closing: bool,
}

#[derive(Resource, Clone)]
pub struct BrowserLifecycleResource(pub Arc<Mutex<BrowserLifecycleInner>>);

#[derive(Default)]
pub struct BrowserCloseGuardsInner {
    pub allow_next_windowless_do_close: bool,
    pub bypass_windowless_do_close_guard: bool,
}

#[derive(Resource, Clone)]
pub struct BrowserCloseGuardsResource(pub Arc<Mutex<BrowserCloseGuardsInner>>);

#[derive(Event, Debug, Clone, Copy)]
pub struct RequestCloseAllBrowsersEvent {
    pub force_close: bool,
}

pub fn apply_close_all_browsers_requests_system(
    mut events: EventReader<RequestCloseAllBrowsersEvent>,
    mut close_all_events: EventWriter<crate::browser::event::CloseAllBrowsersBrowserEvent>,
) {
    let mut n = 0u32;
    for ev in events.read() {
        n += 1;
        close_all_events.send(crate::browser::event::CloseAllBrowsersBrowserEvent {
            force_close: ev.force_close,
        });
    }
    if n > 0 {
        crate::log::record_runtime_event(&format!(
            "apply_close_all_browsers_requests_system forwarded n={n} to CloseAllBrowsersBrowserEvent"
        ));
    }
}
