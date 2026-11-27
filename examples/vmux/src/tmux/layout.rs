//! Manual / tmux BSP tiling: split anchor, pending split merge, and pane-tree rect computation.

use std::collections::{HashMap, HashSet};

use cef::ImplBrowser;
use winit::window::WindowId;

use crate::browser::cef::shell::OsrHostState;
use crate::runtime::RuntimeState;
use crate::window::tiling::PhysRect;

use super::pane_tree::{WmPaneTree, build_dwindle_chain, rects_for_tree};

/// When a split spawn finishes attaching, merge the new [`WindowId`] into [`RuntimeState::wm_pane_tree`].
pub fn on_shell_window_attached(rt: &mut RuntimeState, new_wid: WindowId) {
    let Some(pending) = rt.wm_pending_split.take() else {
        return;
    };
    bevy_log::info!(
        target: "vmux_wm",
        pid = std::process::id(),
        ?new_wid,
        ?pending.anchor,
        vertical_bar = pending.vertical_bar,
        "on_shell_window_attached: merged BSP pane for split spawn"
    );
    let existing = rt.wm_pane_tree.take();
    rt.wm_pane_tree = Some(match existing {
        None => WmPaneTree::Split {
            vertical_bar: pending.vertical_bar,
            ratio: 0.5,
            a: Box::new(WmPaneTree::Leaf(pending.anchor)),
            b: Box::new(WmPaneTree::Leaf(new_wid)),
        },
        Some(t) => t.split_at_anchor(pending.anchor, new_wid, pending.vertical_bar),
    });
    rt.wm_focused_shell_window = Some(new_wid);
}

/// Focused shell for the next split, else lowest browser id.
pub fn split_anchor_window_id(osr_host: &OsrHostState, rt: &RuntimeState) -> Option<WindowId> {
    let guard = osr_host.cef_attach.windows_store.lock().ok()?;
    if let Some(fw) = rt.wm_focused_shell_window {
        if guard.contains_key(&fw) {
            return Some(fw);
        }
    }
    let mut pairs: Vec<(WindowId, i32)> = guard
        .iter()
        .map(|(w, e)| (*w, e.browser.identifier()))
        .collect();
    pairs.sort_by_key(|(_, bid)| *bid);
    pairs.first().map(|(w, _)| *w)
}

/// Prune/rebuild [`RuntimeState::wm_pane_tree`] and return BSP rects, or `None` to fall back to generic tiling.
pub fn bsp_pane_rects_or_fallback(
    rt: &mut RuntimeState,
    valid: &HashSet<WindowId>,
    wids_sorted: &[WindowId],
    work: PhysRect,
) -> Option<HashMap<WindowId, PhysRect>> {
    rt.wm_pane_tree = rt
        .wm_pane_tree
        .take()
        .and_then(|t| t.prune_to_valid(valid));

    let n = wids_sorted.len();
    let count_ok = rt
        .wm_pane_tree
        .as_ref()
        .map(|t| t.leaf_count() == n)
        .unwrap_or(false);

    if !count_ok {
        rt.wm_pane_tree = build_dwindle_chain(wids_sorted, rt.wm_dwindle_axis_phase);
    }

    let tree = rt.wm_pane_tree.as_ref()?;
    Some(rects_for_tree(tree, work))
}
