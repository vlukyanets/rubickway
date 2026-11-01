//! Rendering tests: the grid is rendered offscreen on Mesa's software rasteriser (llvmpipe),
//! read back, and checked against pixel facts computed from the inputs (constitution III).
//! No reference images are stored.
#![expect(
    clippy::panic,
    reason = "the render helper fails loudly; clippy.toml only exempts #[test] functions"
)]

use std::sync::Mutex;

use rubickway::canvas::{Camera, Point, Size};
use rubickway::render::{self, AXIS_ALPHA, BACKGROUND, GridParams, MAJOR_ALPHA, MINOR_ALPHA};

/// EGL contexts are created per render; one at a time keeps the tests independent.
static GPU: Mutex<()> = Mutex::new(());

const SIZE: Size = Size { w: 640.0, h: 400.0 };
/// Allowed difference per colour channel (rounding, rasteriser differences).
const TOLERANCE: u8 = 2;

struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }
}

fn render(camera: Camera, scale: f64) -> (GridParams, Image) {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let params = GridParams::new(&camera, SIZE, scale);
    let width = (SIZE.w * scale).round() as u32;
    let height = (SIZE.h * scale).round() as u32;
    let rgba = match render::offscreen(&params, width, height) {
        Ok(rgba) => rgba,
        Err(err) => panic!("offscreen render failed: {err}"),
    };
    (
        params,
        Image {
            width,
            height,
            rgba,
        },
    )
}

/// White at `alpha` over the background, as 8-bit RGB.
fn over_background(alpha: f32) -> [u8; 3] {
    BACKGROUND.map(|c| ((c + (1.0 - c) * alpha) * 255.0).round() as u8)
}

fn assert_rgb(img: &Image, x: u32, y: u32, expected: [u8; 3], what: &str) {
    let got = img.rgb(x, y);
    let ok = got
        .iter()
        .zip(expected)
        .all(|(g, e)| g.abs_diff(e) <= TOLERANCE);
    assert!(
        ok,
        "{what}: pixel ({x}, {y}) is {got:?}, expected {expected:?} +-{TOLERANCE}"
    );
}

/// Opacity of the grid line with index `n` (counted from the super-major line at the offset).
fn tier_alpha(n: i64, fade: f32) -> f32 {
    if n.rem_euclid(16) == 0 {
        MAJOR_ALPHA
    } else if n.rem_euclid(4) == 0 {
        MINOR_ALPHA + (MAJOR_ALPHA - MINOR_ALPHA) * fade
    } else {
        MINOR_ALPHA * fade
    }
}

/// Checks every pixel of row `y` (which must not lie on a horizontal line) when all vertical
/// line positions are whole pixels: line columns have their tier colour, the vertical axis has
/// the axis colour, everything else is background.
fn assert_crisp_row(img: &Image, params: &GridParams, y: u32) {
    assert_eq!(
        params.minor_px.fract(),
        0.0,
        "scene must have whole-pixel spacing"
    );
    assert_eq!(
        params.offset_px[0].fract(),
        0.0,
        "scene must have whole-pixel offset"
    );
    let minor = params.minor_px as i64;
    let offset = params.offset_px[0] as i64;
    let axis = params.axis_px[0].map(|a| a as i64);
    for x in 0..img.width {
        let rel = i64::from(x) - offset;
        let mut alpha = 0.0f32;
        if rel.rem_euclid(minor) == 0 {
            alpha = tier_alpha(rel.div_euclid(minor), params.minor_alpha);
        }
        if axis == Some(i64::from(x)) {
            alpha = alpha.max(AXIS_ALPHA);
        }
        assert_rgb(img, x, y, over_background(alpha), "grid row");
    }
}

/// Columns of row `y` brighter than the background by more than `threshold` levels, merged
/// into clusters and returned as brightness-weighted centres.
fn line_centres(img: &Image, y: u32, threshold: u8) -> Vec<f64> {
    let bg = over_background(0.0)[0];
    let mut centres = Vec::new();
    let mut cluster: Vec<(u32, f64)> = Vec::new();
    for x in 0..=img.width {
        let lift = if x < img.width {
            img.rgb(x, y)[0].saturating_sub(bg)
        } else {
            0
        };
        if lift > threshold {
            cluster.push((x, f64::from(lift)));
        } else if !cluster.is_empty() {
            let total: f64 = cluster.iter().map(|c| c.1).sum();
            let centre = cluster.iter().map(|c| f64::from(c.0) * c.1).sum::<f64>() / total;
            centres.push(centre);
            cluster.clear();
        }
    }
    centres
}

#[test]
fn renders_on_llvmpipe() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let name = match render::offscreen_renderer_name() {
        Ok(name) => name,
        Err(err) => panic!("cannot query the renderer: {err}"),
    };
    assert!(
        name.contains("llvmpipe"),
        "pixel tests need llvmpipe, got {name:?}"
    );
}

#[test]
fn background_between_lines() {
    let (_, img) = render(Camera::default(), 1.0);
    for (x, y) in [(5, 5), (100, 100), (630, 390), (333, 222)] {
        assert_rgb(&img, x, y, over_background(0.0), "background");
    }
}

#[test]
fn axes_cross_at_the_window_centre_one_pixel_wide() {
    let (_, img) = render(Camera::default(), 1.0);
    let axis = over_background(AXIS_ALPHA);
    let bg = over_background(0.0);
    assert_rgb(&img, 320, 100, axis, "vertical axis");
    assert_rgb(&img, 319, 100, bg, "left of the vertical axis");
    assert_rgb(&img, 321, 100, bg, "right of the vertical axis");
    assert_rgb(&img, 100, 200, axis, "horizontal axis");
    assert_rgb(&img, 100, 199, bg, "above the horizontal axis");
    assert_rgb(&img, 100, 201, bg, "below the horizontal axis");
}

#[test]
fn canvas_y_points_down() {
    // The camera looks 50 units further down, so the origin moves 50 px up on screen.
    let camera = Camera {
        center: Point::new(0.0, 50.0),
        zoom: 1.0,
    };
    let (_, img) = render(camera, 1.0);
    assert_rgb(
        &img,
        100,
        150,
        over_background(AXIS_ALPHA),
        "horizontal axis",
    );
    assert_rgb(&img, 100, 250, over_background(0.0), "mirror position");
}

#[test]
fn lines_every_32_px_with_tiers_at_100_percent() {
    let (params, img) = render(Camera::default(), 1.0);
    assert_eq!(params.minor_px, 32.0);
    assert_crisp_row(&img, &params, 100);
}

#[test]
fn lines_every_20_px_at_1000_percent() {
    let camera = Camera {
        center: Point::new(0.0, 0.0),
        zoom: 10.0,
    };
    let (params, img) = render(camera, 1.0);
    assert_eq!(params.minor_px, 20.0);
    // Horizontal lines sit at 200 + 20k; row 10 is between them.
    assert_crisp_row(&img, &params, 10);
}

#[test]
fn faded_grid_at_10_percent_is_not_a_solid_fill() {
    let camera = Camera {
        center: Point::new(0.0, 0.0),
        zoom: 0.1,
    };
    let (params, img) = render(camera, 1.0);
    let row = 100;
    let bg = over_background(0.0)[0];
    let lit = (0..img.width)
        .filter(|&x| img.rgb(x, row)[0] > bg + TOLERANCE)
        .count();
    assert!(
        lit * 4 < img.width as usize,
        "{lit} of {} pixels lit: looks like a fill",
        img.width
    );
    // Minor lines (12.8 px) are almost faded out; major lines every 51.2 px remain visible.
    let centres = line_centres(&img, row, 5);
    assert!(centres.len() >= 10, "too few visible lines: {centres:?}");
    let major = f64::from(params.major_px);
    for pair in centres.windows(2) {
        let gap = pair[1] - pair[0];
        let steps = (gap / major).round();
        assert!(
            steps >= 1.0 && (gap - steps * major).abs() < 1.0,
            "line gap {gap} is not a multiple of {major}"
        );
    }
}

#[test]
fn one_pixel_lines_at_scale_1_5() {
    let (params, img) = render(Camera::default(), 1.5);
    assert_eq!((img.width, img.height), (960, 600));
    assert_eq!(params.minor_px, 48.0);
    assert_eq!(params.axis_px, [Some(480.0), Some(300.0)]);
    // Horizontal lines sit at 300 + 48k; row 100 is between them.
    assert_crisp_row(&img, &params, 100);
}

#[test]
fn no_jitter_a_million_units_away() {
    // Both views are off the axes. Shifting the camera by a whole number of 16-line periods
    // must not change a single pixel beyond the tolerance.
    let near = Camera {
        center: Point::new(1037.5, -1012.25),
        zoom: 1.7,
    };
    let (params, near_img) = render(near, 1.0);
    let period = params.minor_units * 16.0;
    let shift = (1e6 / period).round() * period;
    let far = Camera {
        center: Point::new(near.center.x + shift, near.center.y - shift),
        zoom: 1.7,
    };
    let (_, far_img) = render(far, 1.0);
    for y in 0..near_img.height {
        for x in 0..near_img.width {
            assert_rgb(&far_img, x, y, near_img.rgb(x, y), "far view vs near view");
        }
    }
}
