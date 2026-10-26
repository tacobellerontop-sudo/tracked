//! World → minimap transform.
//!
//! valorant-api.com publishes four constants per map that turn a VALORANT
//! world position into a normalized (0..1) point on that map's top-down
//! `displayIcon` image. Note the axis swap: the image's horizontal axis comes
//! from world **y**, its vertical axis from world **x**.

use super::Loc;

/// Per-map calibration from `https://valorant-api.com/v1/maps`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MapCal {
    pub x_multiplier: f32,
    pub y_multiplier: f32,
    pub x_scalar_to_add: f32,
    pub y_scalar_to_add: f32,
}

impl MapCal {
    /// `None` for maps without calibration (all four constants zero), e.g.
    /// the Range or a map valorant-api hasn't calibrated yet.
    pub fn new(x_mul: f32, y_mul: f32, x_add: f32, y_add: f32) -> Option<MapCal> {
        let cal = MapCal {
            x_multiplier: x_mul,
            y_multiplier: y_mul,
            x_scalar_to_add: x_add,
            y_scalar_to_add: y_add,
        };
        (x_mul != 0.0 && y_mul != 0.0).then_some(cal)
    }
}

/// Normalized minimap position (0..1 on both axes, origin top-left).
pub fn world_to_norm(l: &Loc, c: &MapCal) -> (f32, f32) {
    let nx = l.y * c.x_multiplier + c.x_scalar_to_add;
    let ny = l.x * c.y_multiplier + c.y_scalar_to_add;
    (nx, ny)
}

/// Unit vector on the minimap for a world-space facing angle.
///
/// `view_radians` is a yaw in world space: the direction `(cos θ, sin θ)` in
/// world (x, y). Pushing that vector through the same linear part of the
/// transform as positions keeps facing consistent with the axis swap and the
/// per-map signs, instead of guessing a zero direction per map.
pub fn facing(view_radians: f32, c: &MapCal) -> (f32, f32) {
    let (wx, wy) = (view_radians.cos(), view_radians.sin());
    // Only the signs matter; magnitudes are equal on every map (square images).
    let dx = wy * c.x_multiplier.signum();
    let dy = wx * c.y_multiplier.signum();
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    (dx / len, dy / len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_multipliers_mean_uncalibrated() {
        assert!(MapCal::new(0.0, 0.0, 0.0, 0.0).is_none());
        assert!(MapCal::new(7e-5, -7e-5, 0.8, 0.5).is_some());
    }

    #[test]
    fn facing_is_unit_length() {
        let c = MapCal::new(7e-5, -7e-5, 0.8, 0.5).unwrap();
        for i in 0..16 {
            let (x, y) = facing(i as f32 * 0.4, &c);
            assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-4);
        }
    }
}
