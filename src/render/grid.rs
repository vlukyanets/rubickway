//! Grid parameters: everything the grid shader needs, computed in `f64` on the CPU.
//!
//! Spacing levels are `8 * 4^k` canvas units. The shader draws three tiers: minor lines
//! (faded in with `minor_alpha`), major lines every 4 minor lines and super-major lines every
//! 16. Because each level's major lines are the next level's minor lines, the tiers hand over
//! without popping when the level changes. All values passed to the shader are small physical
//! pixel quantities: the position of the canvas origin is reduced modulo one 16-line period in
//! `f64` first, so the picture does not jitter far from the origin.

use crate::canvas::{Camera, Size};

/// Background colour, `#1d1f24`.
pub const BACKGROUND: [f32; 3] = [
    0x1d as f32 / 255.0,
    0x1f as f32 / 255.0,
    0x24 as f32 / 255.0,
];
/// Line opacity (white over the background) for minor lines at full fade.
pub const MINOR_ALPHA: f32 = 0.05;
/// Line opacity for major (and super-major) lines.
pub const MAJOR_ALPHA: f32 = 0.10;
/// Line opacity for the two origin axes.
pub const AXIS_ALPHA: f32 = 0.22;

/// Minimum on-screen spacing of minor lines, in logical pixels.
const MIN_SPACING: f64 = 12.0;
/// Spacing at which minor lines are fully faded in, in logical pixels.
const FULL_SPACING: f64 = 24.0;
/// Minor lines per super-major period.
const PERIOD_LINES: f64 = 16.0;

/// Shader inputs for one frame of one viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridParams {
    /// Minor spacing in canvas units (`8 * 4^k`).
    pub minor_units: f64,
    /// Minor spacing in physical pixels.
    pub minor_px: f32,
    /// Major spacing in physical pixels (`4 * minor_px`).
    pub major_px: f32,
    /// Physical-pixel position of a super-major line, in `[0, period_px)`, per axis (x, y).
    pub offset_px: [f32; 2],
    /// Minor line fade, 0..1.
    pub minor_alpha: f32,
    /// Physical-pixel position of the vertical (x = 0) and horizontal (y = 0) origin axes, when
    /// on screen.
    pub axis_px: [Option<f32>; 2],
}

impl GridParams {
    /// Computes the grid for `camera` in a viewer of logical `size` at buffer `scale`.
    pub fn new(camera: &Camera, size: Size, scale: f64) -> GridParams {
        let zoom = camera.zoom;
        let minor_units = minor_spacing_units(zoom);
        let minor_logical = minor_units * zoom;
        let minor_px = minor_logical * scale;
        let period_px = minor_px * PERIOD_LINES;

        // Physical position of the canvas origin; y points down on screen and on the canvas.
        let origin = [
            (size.w / 2.0 - camera.center.x * zoom) * scale,
            (size.h / 2.0 - camera.center.y * zoom) * scale,
        ];
        let extent = [size.w * scale, size.h * scale];

        let offset_px = origin.map(|o| {
            let reduced = o.rem_euclid(period_px) as f32;
            if reduced >= period_px as f32 {
                0.0
            } else {
                reduced
            }
        });
        let axis_px = [0, 1].map(|i| {
            (origin[i] >= -1.0 && origin[i] <= extent[i] + 1.0).then_some(origin[i] as f32)
        });
        let minor_alpha =
            ((minor_logical - MIN_SPACING) / (FULL_SPACING - MIN_SPACING)).clamp(0.0, 1.0) as f32;

        GridParams {
            minor_units,
            minor_px: minor_px as f32,
            major_px: (minor_px * 4.0) as f32,
            offset_px,
            minor_alpha,
            axis_px,
        }
    }

    /// Length of one super-major period (16 minor lines) in physical pixels.
    pub fn period_px(&self) -> f32 {
        self.minor_px * PERIOD_LINES as f32
    }
}

/// The smallest `8 * 4^k` (integer k) whose on-screen size at `zoom` is at least
/// `MIN_SPACING` logical pixels.
fn minor_spacing_units(zoom: f64) -> f64 {
    let level = |k: i32| 8.0 * 4f64.powi(k);
    let mut k = ((MIN_SPACING / 8.0 / zoom).ln() / 4f64.ln()).ceil() as i32;
    while level(k - 1) * zoom >= MIN_SPACING {
        k -= 1;
    }
    while level(k) * zoom < MIN_SPACING {
        k += 1;
    }
    level(k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Camera, Point, Size};

    const SIZE: Size = Size { w: 640.0, h: 400.0 };

    fn camera(x: f64, y: f64, zoom: f64) -> Camera {
        Camera {
            center: Point::new(x, y),
            zoom,
        }
    }

    #[test]
    fn minor_spacing_is_smallest_power_of_four_step_of_at_least_12_px() {
        // zoom 1: 8 * 4^k >= 12 -> k = 1 -> 32 units = 32 px.
        let p = GridParams::new(&camera(0.0, 0.0, 1.0), SIZE, 1.0);
        assert_eq!(p.minor_units, 32.0);
        assert_eq!(p.minor_px, 32.0);
        // zoom 0.1: 8 * 4^k * 0.1 >= 12 -> k = 2 -> 128 units = 12.8 px.
        let p = GridParams::new(&camera(0.0, 0.0, 0.1), SIZE, 1.0);
        assert_eq!(p.minor_units, 128.0);
        // zoom 10: 8 * 4^k * 10 >= 12 -> k = -1 -> 2 units = 20 px.
        let p = GridParams::new(&camera(0.0, 0.0, 10.0), SIZE, 1.0);
        assert_eq!(p.minor_units, 2.0);
        assert_eq!(p.minor_px, 20.0);
    }

    #[test]
    fn minor_spacing_stays_between_12_and_48_logical_px_over_the_zoom_range() {
        let mut zoom = 0.1;
        while zoom <= 10.0 {
            let p = GridParams::new(&camera(0.0, 0.0, zoom), SIZE, 1.0);
            let logical = p.minor_units * zoom;
            assert!(
                (12.0..48.0 + 1e-9).contains(&logical),
                "zoom {zoom}: {logical}"
            );
            zoom *= 1.03;
        }
    }

    #[test]
    fn spacings_scale_with_buffer_scale() {
        let p = GridParams::new(&camera(0.0, 0.0, 1.0), SIZE, 1.5);
        assert_eq!(p.minor_px, 48.0);
        assert_eq!(p.major_px, 4.0 * p.minor_px);
        assert!(p.minor_px >= 12.0 * 1.5);
    }

    #[test]
    fn minor_alpha_fades_between_12_and_24_logical_px() {
        // zoom 0.375 -> 32 units * 0.375 = 12 px -> alpha 0.
        let p = GridParams::new(&camera(0.0, 0.0, 0.375), SIZE, 1.0);
        assert!(p.minor_alpha.abs() < 1e-6);
        // zoom 0.75 -> 24 px -> alpha 1.
        let p = GridParams::new(&camera(0.0, 0.0, 0.75), SIZE, 1.0);
        assert!((p.minor_alpha - 1.0).abs() < 1e-6);
        // zoom 0.5625 -> 18 px -> alpha 0.5.
        let p = GridParams::new(&camera(0.0, 0.0, 0.5625), SIZE, 1.0);
        assert!((p.minor_alpha - 0.5).abs() < 1e-6);
    }

    #[test]
    fn offset_is_reduced_into_one_period() {
        for (x, y, zoom) in [(0.0, 0.0, 1.0), (123.4, -567.8, 0.37), (1e6, -1e6, 1.7)] {
            let p = GridParams::new(&camera(x, y, zoom), SIZE, 1.25);
            for o in p.offset_px {
                assert!(
                    o >= 0.0 && o < p.period_px(),
                    "{o} not in [0, {})",
                    p.period_px()
                );
            }
        }
    }

    #[test]
    fn offset_far_away_matches_the_equivalent_view_near_the_origin() {
        // Moving the camera by a whole number of periods (in canvas units) must not change the
        // picture; checks that the reduction happens in f64.
        let zoom = 1.7;
        let near = GridParams::new(&camera(37.5, -12.25, zoom), SIZE, 1.0);
        let period_units = near.minor_units * 16.0;
        let shift = (1e6 / period_units).round() * period_units;
        let far = GridParams::new(&camera(37.5 + shift, -12.25 - shift, zoom), SIZE, 1.0);
        for i in 0..2 {
            assert!((near.offset_px[i] - far.offset_px[i]).abs() < 1e-3);
        }
    }

    #[test]
    fn origin_axes_only_when_on_screen() {
        let p = GridParams::new(&camera(0.0, 0.0, 1.0), SIZE, 1.0);
        assert_eq!(p.axis_px, [Some(320.0), Some(200.0)]);
        let p = GridParams::new(&camera(10_000.0, 0.0, 1.0), SIZE, 1.0);
        assert_eq!(p.axis_px[0], None);
        assert_eq!(p.axis_px[1], Some(200.0));
    }
}
