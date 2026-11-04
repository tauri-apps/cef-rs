use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Commands, Query, ResMut, Resource};
use winit::window::Window;
use winit::window::WindowId;

static TRACKED: Mutex<Vec<Weak<Window>>> = Mutex::new(Vec::new());

pub fn track_window(window: &Arc<Window>) {
    let mut g = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|w| w.strong_count() > 0);
    g.push(Arc::downgrade(window));
}

pub fn show_all_windows() {
    let g = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    for w in g.iter().filter_map(Weak::upgrade) {
        w.set_visible(true);
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct WindowComponent {
    pub window_id: WindowId,
    pub browser_id: i32,
}

#[derive(Resource, Default)]
pub(crate) struct WindowRegistryState {
    entities_by_window: HashMap<WindowId, Entity>,
}

pub(crate) fn sync_window_components_with_cef(
    mut commands: Commands,
    mut state: ResMut<WindowRegistryState>,
    mut window_components: Query<&mut WindowComponent>,
) {
    let Some(idx) = crate::runtime::try_ffi_osr_index() else {
        return;
    };
    let pairs = crate::browser::cef::osr::browser_window_pairs(idx.as_ref());
    let mut seen = HashSet::new();

    for (browser_id, window_id) in pairs {
        seen.insert(window_id);
        if let Some(entity) = state.entities_by_window.get(&window_id).copied() {
            if let Ok(mut component) = window_components.get_mut(entity) {
                component.browser_id = browser_id;
            } else {
                let entity = commands
                    .spawn(WindowComponent {
                        window_id,
                        browser_id,
                    })
                    .id();
                state.entities_by_window.insert(window_id, entity);
            }
            continue;
        }
        let entity = commands
            .spawn(WindowComponent {
                window_id,
                browser_id,
            })
            .id();
        state.entities_by_window.insert(window_id, entity);
    }

    let stale: Vec<WindowId> = state
        .entities_by_window
        .keys()
        .copied()
        .filter(|window_id| !seen.contains(window_id))
        .collect();

    for window_id in stale {
        if let Some(entity) = state.entities_by_window.remove(&window_id) {
            commands.entity(entity).despawn();
        }
    }
}
