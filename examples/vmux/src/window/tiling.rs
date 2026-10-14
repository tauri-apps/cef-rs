//! Pure physical-pixel tiling geometry (dwindle, grid, stack).

/// Work area / tile rectangle in **physical** pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

const MIN_TILE: u32 = 128;

pub(crate) fn split_vertical(r: PhysRect, ratio: f32) -> (PhysRect, PhysRect) {
    let ratio = ratio.clamp(0.15, 0.85);
    let w0 = ((r.w as f32) * ratio).round() as u32;
    let w0 = w0.max(MIN_TILE).min(r.w.saturating_sub(MIN_TILE));
    let w1 = r.w.saturating_sub(w0);
    (
        PhysRect {
            x: r.x,
            y: r.y,
            w: w0,
            h: r.h,
        },
        PhysRect {
            x: r.x + w0 as i32,
            y: r.y,
            w: w1,
            h: r.h,
        },
    )
}

pub(crate) fn split_horizontal(r: PhysRect, ratio: f32) -> (PhysRect, PhysRect) {
    let ratio = ratio.clamp(0.15, 0.85);
    let h0 = ((r.h as f32) * ratio).round() as u32;
    let h0 = h0.max(MIN_TILE).min(r.h.saturating_sub(MIN_TILE));
    let h1 = r.h.saturating_sub(h0);
    (
        PhysRect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: h0,
        },
        PhysRect {
            x: r.x,
            y: r.y + h0 as i32,
            w: r.w,
            h: h1,
        },
    )
}

/// Dwindle-style tiling: repeatedly split the **last** rectangle; axis alternates starting with
/// `phase_offset` (0 = first split vertical bar / side-by-side).
pub fn dwindle_tile_rects(work: PhysRect, n: usize, phase_offset: u32) -> Vec<PhysRect> {
    if n == 0 {
        return Vec::new();
    }
    let mut rects = Vec::with_capacity(n);
    rects.push(work);
    for i in 1..n {
        let idx = rects.len() - 1;
        let r = rects[idx];
        let vertical = ((i as u32 + phase_offset) % 2) == 1;
        let (a, b) = if vertical {
            split_vertical(r, 0.5)
        } else {
            split_horizontal(r, 0.5)
        };
        rects[idx] = a;
        rects.push(b);
    }
    rects
}

/// Rough √N grid.
pub fn grid_tile_rects(work: PhysRect, n: usize) -> Vec<PhysRect> {
    if n == 0 {
        return Vec::new();
    }
    let cols = (n as f64).sqrt().ceil() as usize;
    let cols = cols.max(1);
    let rows = (n + cols - 1) / cols;
    let cell_w = work.w / cols as u32;
    let cell_h = work.h / rows as u32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let col = i % cols;
        let row = i / cols;
        let x = work.x + (col as u32 * cell_w) as i32;
        let y = work.y + (row as u32 * cell_h) as i32;
        let mut w = cell_w;
        let mut h = cell_h;
        if col == cols - 1 {
            w = work.w.saturating_sub(cell_w * (cols as u32 - 1));
        }
        if row == rows - 1 {
            h = work.h.saturating_sub(cell_h * (rows as u32 - 1));
        }
        out.push(PhysRect { x, y, w, h });
    }
    out
}

#[cfg(test)]
use bevy_ecs::prelude::{Res, ResMut, Resource, World};
#[cfg(test)]
use bevy_ecs::system::RunSystemOnce;

#[cfg(test)]
#[derive(Resource)]
struct DwindleTileCase {
    work: PhysRect,
    n: usize,
    phase: u32,
}

#[cfg(test)]
#[derive(Resource, Default)]
struct DwindleTileOut(Vec<PhysRect>);

#[cfg(test)]
fn dwindle_tile_rects_system(inp: Res<DwindleTileCase>, mut out: ResMut<DwindleTileOut>) {
    out.0 = dwindle_tile_rects(inp.work, inp.n, inp.phase);
}

#[cfg(test)]
#[test]
fn dwindle_one_is_full_via_system() {
    let w = PhysRect {
        x: 0,
        y: 0,
        w: 1000,
        h: 800,
    };
    let mut world = World::default();
    world.insert_resource(DwindleTileCase {
        work: w,
        n: 1,
        phase: 0,
    });
    world.insert_resource(DwindleTileOut::default());
    world.run_system_once(dwindle_tile_rects_system).unwrap();
    let r = &world.resource::<DwindleTileOut>().0;
    assert_eq!(r.len(), 1);
    assert_eq!(r[0], w);
}

#[cfg(test)]
#[test]
fn dwindle_two_splits_vertical_first_when_phase_0_via_system() {
    let w = PhysRect {
        x: 0,
        y: 0,
        w: 1000,
        h: 800,
    };
    let mut world = World::default();
    world.insert_resource(DwindleTileCase {
        work: w,
        n: 2,
        phase: 0,
    });
    world.insert_resource(DwindleTileOut::default());
    world.run_system_once(dwindle_tile_rects_system).unwrap();
    let r = &world.resource::<DwindleTileOut>().0;
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].y, 0);
    assert_eq!(r[1].y, 0);
    assert_eq!(r[0].h, 800);
    assert_eq!(r[1].h, 800);
    assert!(r[0].w + r[1].w >= w.w - 2);
}

#[cfg(test)]
#[derive(Resource)]
struct GridTileCase {
    work: PhysRect,
    n: usize,
}

#[cfg(test)]
#[derive(Resource, Default)]
struct GridTileOut(Vec<PhysRect>);

#[cfg(test)]
fn grid_tile_rects_system(inp: Res<GridTileCase>, mut out: ResMut<GridTileOut>) {
    out.0 = grid_tile_rects(inp.work, inp.n);
}

#[cfg(test)]
#[test]
fn grid_four_is_two_by_two_via_system() {
    let w = PhysRect {
        x: 10,
        y: 20,
        w: 400,
        h: 200,
    };
    let mut world = World::default();
    world.insert_resource(GridTileCase { work: w, n: 4 });
    world.insert_resource(GridTileOut::default());
    world.run_system_once(grid_tile_rects_system).unwrap();
    let r = &world.resource::<GridTileOut>().0;
    assert_eq!(r.len(), 4);
    assert_eq!(r[0].x, 10);
    assert_eq!(r[0].y, 20);
    assert_eq!(r[2].y, 20 + r[0].h as i32);
}
