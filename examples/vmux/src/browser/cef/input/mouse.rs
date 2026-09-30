use cef::sys::cef_event_flags_t;
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;

pub fn cursor_moved_dip(position_physical: PhysicalPosition<f64>, scale_factor: f64) -> (i32, i32) {
    let logical = position_physical.to_logical::<f64>(scale_factor);
    (logical.x.round() as i32, logical.y.round() as i32)
}

pub fn take_wheel_deltas_i32(residual: &mut (f64, f64), dx: f64, dy: f64) -> Option<(i32, i32)> {
    residual.0 += dx;
    residual.1 += dy;
    let out_x = residual.0.round() as i32;
    let out_y = residual.1.round() as i32;
    residual.0 -= out_x as f64;
    residual.1 -= out_y as f64;
    if out_x == 0 && out_y == 0 {
        None
    } else {
        Some((out_x, out_y))
    }
}

pub fn wheel_to_cef(
    delta: MouseScrollDelta,
    mods: cef_event_flags_t,
) -> (f64, f64, cef_event_flags_t) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (x as f64 * 120.0, y as f64 * 120.0, mods),
        MouseScrollDelta::PixelDelta(p) => {
            // Physical pixels; mark as precision scrolling.
            (
                p.x,
                p.y,
                mods | cef_event_flags_t::EVENTFLAG_PRECISION_SCROLLING_DELTA,
            )
        }
    }
}
