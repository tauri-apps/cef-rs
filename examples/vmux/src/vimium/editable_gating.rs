//! Editable-focus snapshot and gating for app-level keyboard UX (browse vs insert).
//! Built from ECS [`crate::browser::cef::facet::EditableFocusHint`] in
//! [`crate::window::system::apply_osr_host_window_dispatches_system`] and vimium replay paths.
//!
//! **Link hints (`f`), plain scroll (`j`/`k`/…), and mode chords (`/`, `i`, …)** intentionally do **not** use
//! [`page_allows_app_shortcuts`]: Google-style pages autofocus search and the probe reports typing,
//! which would block those keys on first paint (see `window_input`).
//!
//! **Shift+letter** actions (**Shift+H/L**, **Shift+R** reload, **Shift+G** bottom, **Shift+N** find-next, …)
//! use [`may_handle_history_shortcuts`]: yield to the page only when the probe is **sure** focus is in a
//! text control. Plain scroll still uses [`pass_letter_like_vimium_binding_to_page`] so macOS can attach
//! `text` to `j`/`k` off-input without starving scroll until the probe runs.
//!
//! OS shortcuts (e.g. **Cmd+[** / **Cmd+R**) live in `window::dispatch` and do not use this snapshot.

pub use crate::browser::cef::facet::EditableFocusSnapshot;

pub(crate) fn may_handle_history_shortcuts(hints: &EditableFocusSnapshot, browser_id: i32) -> bool {
    match hints.get(&browser_id) {
        None | Some(None) | Some(Some(false)) => true,
        Some(Some(true)) => false,
    }
}

/// Page is in “browse” mode for app shortcuts (not focused in a text control).
pub(crate) fn page_allows_app_shortcuts(hints: &EditableFocusSnapshot, browser_id: i32) -> bool {
    matches!(hints.get(&browser_id), Some(Some(false)))
}

pub(crate) fn editable_focus_is_typing(hints: &EditableFocusSnapshot, browser_id: i32) -> bool {
    crate::browser::cef::facet::editable_focus_is_typing(hints, browser_id)
}

/// DOM probe finished and reported focus is **not** in a text control.
pub(crate) fn editable_focus_probe_says_browse(
    hints: &EditableFocusSnapshot,
    browser_id: i32,
) -> bool {
    matches!(hints.get(&browser_id), Some(Some(false)))
}

/// Letter-like vimium bindings (scroll, reload, find-next): yield to the page when the probe says
/// typing, or when we have no definitive browse signal and winit attached printable `text` (IME /
/// macOS often sets `text` on plain `j`/`k` even off inputs — without this branch we would never
/// scroll; with it we still conservatively pass through while the probe is `None` or the user is
/// typing).
pub(crate) fn pass_letter_like_vimium_binding_to_page(
    hints: &EditableFocusSnapshot,
    browser_id: i32,
    key_sends_printable_text: bool,
) -> bool {
    if editable_focus_is_typing(hints, browser_id) {
        return true;
    }
    if editable_focus_probe_says_browse(hints, browser_id) {
        return false;
    }
    key_sends_printable_text
}

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
use bevy_ecs::prelude::{Res, Resource, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct PageAllowsStep {
    hints: HashMap<i32, Option<bool>>,
    browser_id: i32,
    expect: bool,
}

#[cfg(test)]
fn page_allows_assert_system(case: Res<PageAllowsStep>) {
    assert_eq!(
        page_allows_app_shortcuts(&case.hints, case.browser_id),
        case.expect
    );
}

#[cfg(test)]
#[test]
fn page_allows_app_shortcuts_only_after_probe_says_not_editable_via_system() {
    let mut world = World::default();

    let mut hints = HashMap::new();
    hints.insert(7, None);
    world.insert_resource(PageAllowsStep {
        hints: hints.clone(),
        browser_id: 7,
        expect: false,
    });
    world.run_system_once(page_allows_assert_system).unwrap();

    hints.insert(7, Some(true));
    world.insert_resource(PageAllowsStep {
        hints: hints.clone(),
        browser_id: 7,
        expect: false,
    });
    world.run_system_once(page_allows_assert_system).unwrap();

    hints.insert(7, Some(false));
    world.insert_resource(PageAllowsStep {
        hints,
        browser_id: 7,
        expect: true,
    });
    world.run_system_once(page_allows_assert_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct HistoryGateStep {
    hints: HashMap<i32, Option<bool>>,
    browser_id: i32,
    expect: bool,
}

#[cfg(test)]
fn history_gate_assert_system(case: Res<HistoryGateStep>) {
    assert_eq!(
        may_handle_history_shortcuts(&case.hints, case.browser_id),
        case.expect
    );
}

#[cfg(test)]
#[test]
fn may_handle_history_shortcuts_blocks_only_confirmed_editable_via_system() {
    let mut world = World::default();

    let hints = HashMap::new();
    world.insert_resource(HistoryGateStep {
        hints: hints.clone(),
        browser_id: 1,
        expect: true,
    });
    world.run_system_once(history_gate_assert_system).unwrap();

    let mut hints = hints;
    hints.insert(1, None);
    world.insert_resource(HistoryGateStep {
        hints: hints.clone(),
        browser_id: 1,
        expect: true,
    });
    world.run_system_once(history_gate_assert_system).unwrap();

    hints.insert(1, Some(false));
    world.insert_resource(HistoryGateStep {
        hints: hints.clone(),
        browser_id: 1,
        expect: true,
    });
    world.run_system_once(history_gate_assert_system).unwrap();

    hints.insert(1, Some(true));
    world.insert_resource(HistoryGateStep {
        hints,
        browser_id: 1,
        expect: false,
    });
    world.run_system_once(history_gate_assert_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct PassLetterStep {
    hints: HashMap<i32, Option<bool>>,
    browser_id: i32,
    key_sends_printable_text: bool,
    expect: bool,
}

#[cfg(test)]
fn pass_letter_assert_system(case: Res<PassLetterStep>) {
    assert_eq!(
        pass_letter_like_vimium_binding_to_page(
            &case.hints,
            case.browser_id,
            case.key_sends_printable_text,
        ),
        case.expect
    );
}

#[cfg(test)]
#[test]
fn pass_letter_like_binding_ignores_winit_text_when_probe_says_browse_via_system() {
    let mut world = World::default();

    let mut hints = HashMap::new();
    hints.insert(1, Some(false));
    world.insert_resource(PassLetterStep {
        hints: hints.clone(),
        browser_id: 1,
        key_sends_printable_text: true,
        expect: false,
    });
    world.run_system_once(pass_letter_assert_system).unwrap();

    hints.insert(1, Some(true));
    world.insert_resource(PassLetterStep {
        hints: hints.clone(),
        browser_id: 1,
        key_sends_printable_text: true,
        expect: true,
    });
    world.run_system_once(pass_letter_assert_system).unwrap();

    hints.insert(2, None);
    world.insert_resource(PassLetterStep {
        hints: hints.clone(),
        browser_id: 2,
        key_sends_printable_text: true,
        expect: true,
    });
    world.run_system_once(pass_letter_assert_system).unwrap();

    world.insert_resource(PassLetterStep {
        hints,
        browser_id: 2,
        key_sends_printable_text: false,
        expect: false,
    });
    world.run_system_once(pass_letter_assert_system).unwrap();
}

#[cfg(test)]
#[derive(Resource)]
struct HistoryAndPassLetterCase {
    hints: HashMap<i32, Option<bool>>,
    browser_id: i32,
}

#[cfg(test)]
fn history_and_pass_letter_assert_system(case: Res<HistoryAndPassLetterCase>) {
    assert!(may_handle_history_shortcuts(&case.hints, case.browser_id));
    assert!(pass_letter_like_vimium_binding_to_page(
        &case.hints,
        case.browser_id,
        true,
    ));
}

/// Shift+R reload and similar use [`may_handle_history_shortcuts`], not [`pass_letter_like_vimium_binding_to_page`]:
/// unknown probe + printable `text` must not send the stroke to the page.
#[cfg(test)]
#[test]
fn history_gate_allows_vimium_while_pass_letter_yields_with_unknown_probe_and_text_via_system() {
    let mut world = World::default();
    let mut hints = HashMap::new();
    hints.insert(1, None);
    world.insert_resource(HistoryAndPassLetterCase {
        hints,
        browser_id: 1,
    });
    world
        .run_system_once(history_and_pass_letter_assert_system)
        .unwrap();
}
