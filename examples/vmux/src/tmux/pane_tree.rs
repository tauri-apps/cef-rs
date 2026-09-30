//! BSP pane tree for manual / tmux tiling: splits target the focused shell; layout walks the tree for rects.

use std::collections::{HashMap, HashSet};

use winit::window::WindowId;

use crate::window::tiling::{split_horizontal, split_vertical, PhysRect};

/// Queued before `spawn_cef_browser_window` for a split; applied when the new shell attaches.
#[derive(Debug, Clone, Copy)]
pub struct WmPendingSplit {
    pub anchor: WindowId,
    /// `true` = vertical bar (left | right), tmux `%`. `false` = horizontal bar (top / bottom), tmux `"`.
    pub vertical_bar: bool,
}

#[derive(Debug, Clone)]
pub enum WmPaneTree {
    Leaf(WindowId),
    Split {
        vertical_bar: bool,
        ratio: f32,
        a: Box<WmPaneTree>,
        b: Box<WmPaneTree>,
    },
}

impl WmPaneTree {
    pub fn leaf_count(&self) -> usize {
        match self {
            WmPaneTree::Leaf(_) => 1,
            WmPaneTree::Split { a, b, .. } => a.leaf_count() + b.leaf_count(),
        }
    }

    /// Remove dead windows; collapse unary splits.
    pub fn prune_to_valid(self, valid: &HashSet<WindowId>) -> Option<WmPaneTree> {
        match self {
            WmPaneTree::Leaf(w) => valid.contains(&w).then_some(WmPaneTree::Leaf(w)),
            WmPaneTree::Split {
                vertical_bar,
                ratio,
                a,
                b,
            } => {
                let a = a.prune_to_valid(valid);
                let b = b.prune_to_valid(valid);
                match (a, b) {
                    (None, None) => None,
                    (Some(x), None) | (None, Some(x)) => Some(x),
                    (Some(a), Some(b)) => Some(WmPaneTree::Split {
                        vertical_bar,
                        ratio,
                        a: Box::new(a),
                        b: Box::new(b),
                    }),
                }
            }
        }
    }

    /// Replace `Leaf(anchor)` with a split (`anchor` keeps first child — top/left).
    pub fn split_at_anchor(
        self,
        anchor: WindowId,
        new_wid: WindowId,
        vertical_bar: bool,
    ) -> WmPaneTree {
        if !contains_anchor(&self, anchor) {
            return split_rightmost_leaf(self, new_wid, vertical_bar);
        }
        match self {
            WmPaneTree::Leaf(w) if w == anchor => WmPaneTree::Split {
                vertical_bar,
                ratio: 0.5,
                a: Box::new(WmPaneTree::Leaf(anchor)),
                b: Box::new(WmPaneTree::Leaf(new_wid)),
            },
            WmPaneTree::Leaf(w) => WmPaneTree::Leaf(w),
            WmPaneTree::Split {
                vertical_bar: v,
                ratio: r,
                a,
                b,
            } => {
                if contains_anchor(&a, anchor) {
                    WmPaneTree::Split {
                        vertical_bar: v,
                        ratio: r,
                        a: Box::new(a.split_at_anchor(anchor, new_wid, vertical_bar)),
                        b,
                    }
                } else {
                    WmPaneTree::Split {
                        vertical_bar: v,
                        ratio: r,
                        a,
                        b: Box::new(b.split_at_anchor(anchor, new_wid, vertical_bar)),
                    }
                }
            }
        }
    }
}

fn contains_anchor(node: &WmPaneTree, anchor: WindowId) -> bool {
    match node {
        WmPaneTree::Leaf(w) => *w == anchor,
        WmPaneTree::Split { a, b, .. } => contains_anchor(a, anchor) || contains_anchor(b, anchor),
    }
}

/// Same topology as [`crate::window::tiling::dwindle_tile_rects`]: each new pane splits the rightmost leaf.
pub fn build_dwindle_chain(wids: &[WindowId], phase: u32) -> Option<WmPaneTree> {
    if wids.is_empty() {
        return None;
    }
    let mut node = WmPaneTree::Leaf(wids[0]);
    for i in 1..wids.len() {
        let vertical_bar = ((i as u32 + phase) % 2) == 1;
        node = split_rightmost_leaf(node, wids[i], vertical_bar);
    }
    Some(node)
}

fn split_rightmost_leaf(node: WmPaneTree, new_wid: WindowId, vertical_bar: bool) -> WmPaneTree {
    match node {
        WmPaneTree::Leaf(w) => WmPaneTree::Split {
            vertical_bar,
            ratio: 0.5,
            a: Box::new(WmPaneTree::Leaf(w)),
            b: Box::new(WmPaneTree::Leaf(new_wid)),
        },
        WmPaneTree::Split {
            vertical_bar: v,
            ratio: r,
            a,
            b,
        } => WmPaneTree::Split {
            vertical_bar: v,
            ratio: r,
            a,
            b: Box::new(split_rightmost_leaf(*b, new_wid, vertical_bar)),
        },
    }
}

pub fn rects_for_tree(tree: &WmPaneTree, work: PhysRect) -> HashMap<WindowId, PhysRect> {
    let mut m = HashMap::new();
    assign_rects(tree, work, &mut m);
    m
}

fn assign_rects(node: &WmPaneTree, r: PhysRect, out: &mut HashMap<WindowId, PhysRect>) {
    match node {
        WmPaneTree::Leaf(w) => {
            out.insert(*w, r);
        }
        WmPaneTree::Split {
            vertical_bar,
            ratio,
            a,
            b,
        } => {
            let (ra, rb) = if *vertical_bar {
                split_vertical(r, *ratio)
            } else {
                split_horizontal(r, *ratio)
            };
            assign_rects(a, ra, out);
            assign_rects(b, rb, out);
        }
    }
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut, Resource, World};

#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
fn w(n: u64) -> WindowId {
    WindowId::from(n)
}

#[cfg(test)]
#[derive(Resource)]
struct BspRectsEcsIn {
    tree: WmPaneTree,
    work: PhysRect,
}

#[cfg(test)]
#[derive(Resource, Default)]
struct BspRectsEcsOut(HashMap<WindowId, PhysRect>);

#[cfg(test)]
fn bsp_rects_for_tree_system(inp: Res<BspRectsEcsIn>, mut out: ResMut<BspRectsEcsOut>) {
    out.0 = rects_for_tree(&inp.tree, inp.work);
}

#[cfg(test)]
#[test]
fn split_quote_is_horizontal_bar_top_bottom_via_system() {
    let tree = WmPaneTree::Leaf(w(1)).split_at_anchor(w(1), w(2), false);
    let work = PhysRect {
        x: 0,
        y: 0,
        w: 1000,
        h: 800,
    };
    let mut world = World::default();
    world.insert_resource(BspRectsEcsIn { tree, work });
    world.insert_resource(BspRectsEcsOut::default());
    world.run_system_once(bsp_rects_for_tree_system).unwrap();
    let m = &world.resource::<BspRectsEcsOut>().0;
    let a = m[&w(1)];
    let b = m[&w(2)];
    assert_eq!(a.x, b.x);
    assert_eq!(a.w, b.w);
    assert!(a.y < b.y);
    assert_eq!(a.h + b.h, work.h);
}

#[cfg(test)]
#[test]
fn split_percent_is_vertical_bar_left_right_via_system() {
    let tree = WmPaneTree::Leaf(w(1)).split_at_anchor(w(1), w(2), true);
    let work = PhysRect {
        x: 0,
        y: 0,
        w: 1000,
        h: 800,
    };
    let mut world = World::default();
    world.insert_resource(BspRectsEcsIn { tree, work });
    world.insert_resource(BspRectsEcsOut::default());
    world.run_system_once(bsp_rects_for_tree_system).unwrap();
    let m = &world.resource::<BspRectsEcsOut>().0;
    let a = m[&w(1)];
    let b = m[&w(2)];
    assert_eq!(a.y, b.y);
    assert_eq!(a.h, b.h);
    assert!(a.x < b.x);
    assert_eq!(a.w + b.w, work.w);
}

#[cfg(test)]
#[derive(Resource)]
struct PruneEcsIn {
    tree: WmPaneTree,
    valid: HashSet<WindowId>,
}

#[cfg(test)]
#[derive(Resource, Default)]
struct PruneEcsOut(Option<WmPaneTree>);

#[cfg(test)]
fn prune_pane_tree_system(inp: Res<PruneEcsIn>, mut out: ResMut<PruneEcsOut>) {
    out.0 = inp.tree.clone().prune_to_valid(&inp.valid);
}

#[cfg(test)]
#[test]
fn prune_drops_closed_pane_via_system() {
    let tree = WmPaneTree::Split {
        vertical_bar: true,
        ratio: 0.5,
        a: Box::new(WmPaneTree::Leaf(w(1))),
        b: Box::new(WmPaneTree::Leaf(w(2))),
    };
    let mut valid = HashSet::new();
    valid.insert(w(1));
    let mut world = World::default();
    world.insert_resource(PruneEcsIn { tree, valid });
    world.insert_resource(PruneEcsOut::default());
    world.run_system_once(prune_pane_tree_system).unwrap();
    let p = world
        .resource::<PruneEcsOut>()
        .0
        .as_ref()
        .expect("one leaf");
    assert!(matches!(p, WmPaneTree::Leaf(id) if *id == w(1)));
}
