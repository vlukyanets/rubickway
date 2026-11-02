//! Rendering tests for windows: offscreen on llvmpipe, read back, checked against pixel facts
//! (constitution III, FR-005). Window rectangles are given in physical pixels here; the
//! canvas-to-screen math is tested in the shell and viewer tests.
#![expect(
    clippy::panic,
    reason = "the render helper fails loudly; clippy.toml only exempts #[test] functions"
)]

use std::sync::Mutex;

use rubickway::canvas::{Camera, Point, Size};
use rubickway::render::{self, BACKGROUND, GridParams, ImageRef, Quad, Transform};

static GPU: Mutex<()> = Mutex::new(());

const W: u32 = 320;
const H: u32 = 200;
const TOLERANCE: u8 = 2;

/// A solid `width x height` ARGB image (little-endian bytes B, G, R, A).
fn solid(width: u32, height: u32, argb: u32) -> Vec<u8> {
    (0..width * height)
        .flat_map(|_| argb.to_le_bytes())
        .collect()
}

fn image(bytes: &[u8], width: u32, height: u32, opaque: bool) -> ImageRef<'_> {
    ImageRef {
        bytes,
        width,
        height,
        stride: width * 4,
        opaque,
    }
}

fn quad(key: u64, x: f32, y: f32, w: f32, h: f32) -> Quad {
    Quad {
        key,
        dst: [x, y, w, h],
        src: [0.0, 0.0, 1.0, 1.0],
        transform: Transform::Normal,
    }
}

/// A grid far from the origin, so no axis crosses the scene.
fn grid() -> GridParams {
    let camera = Camera {
        center: Point::new(10_000.5, 10_000.5),
        zoom: 1.0,
    };
    GridParams::new(&camera, Size::new(f64::from(W), f64::from(H)), 1.0)
}

fn render(images: &[(u64, ImageRef<'_>)], quads: &[Quad]) -> Vec<u8> {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    match render::offscreen_scene(&grid(), images, quads, W, H) {
        Ok(rgba) => rgba,
        Err(err) => panic!("offscreen render failed: {err}"),
    }
}

fn rgb(img: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2]]
}

fn assert_rgb(img: &[u8], x: u32, y: u32, expected: [u8; 3], what: &str) {
    let got = rgb(img, x, y);
    let ok = got
        .iter()
        .zip(expected)
        .all(|(g, e)| g.abs_diff(e) <= TOLERANCE);
    assert!(ok, "{what}: ({x}, {y}) is {got:?}, expected {expected:?}");
}

fn background() -> [u8; 3] {
    BACKGROUND.map(|c| (c * 255.0).round() as u8)
}

/// Not a grid line: the grid at this camera has lines at 32 px steps from x = 15.5.
fn is_background(img: &[u8], x: u32, y: u32) -> bool {
    rgb(img, x, y)
        .iter()
        .zip(background())
        .all(|(g, e)| g.abs_diff(e) <= 12)
}

#[test]
fn a_window_covers_exactly_its_rectangle() {
    let red = solid(40, 30, 0xffff_0000);
    let img = render(
        &[(1, image(&red, 40, 30, false))],
        &[quad(1, 100.0, 50.0, 40.0, 30.0)],
    );
    for (x, y) in [(100, 50), (139, 79), (120, 65)] {
        assert_rgb(&img, x, y, [255, 0, 0], "inside");
    }
    for (x, y) in [(99, 60), (140, 60), (120, 49), (120, 80)] {
        assert!(is_background(&img, x, y), "outside ({x}, {y})");
    }
}

#[test]
fn zoom_2_doubles_the_rectangle() {
    let red = solid(40, 30, 0xffff_0000);
    let img = render(
        &[(1, image(&red, 40, 30, false))],
        &[quad(1, 100.0, 50.0, 80.0, 60.0)],
    );
    assert_rgb(&img, 179, 109, [255, 0, 0], "far corner");
    assert!(is_background(&img, 180, 109));
}

#[test]
fn later_quads_are_on_top() {
    let red = solid(10, 10, 0xffff_0000);
    let blue = solid(10, 10, 0xff00_00ff);
    let img = render(
        &[
            (1, image(&red, 10, 10, false)),
            (2, image(&blue, 10, 10, false)),
        ],
        &[
            quad(1, 50.0, 50.0, 40.0, 40.0),
            quad(2, 70.0, 70.0, 40.0, 40.0),
        ],
    );
    assert_rgb(&img, 60, 60, [255, 0, 0], "only red");
    assert_rgb(&img, 80, 80, [0, 0, 255], "overlap shows the top window");
}

#[test]
fn translucent_windows_blend_over_the_grid() {
    // Premultiplied 50% red: bytes B=0, G=0, R=128, A=128.
    let half = solid(10, 10, 0x8080_0000);
    let img = render(
        &[(1, image(&half, 10, 10, false))],
        &[quad(1, 100.0, 100.0, 40.0, 40.0)],
    );
    let bg = background();
    let a = 128.0 / 255.0;
    let expected = [
        (128.0 + f32::from(bg[0]) * (1.0 - a)).round() as u8,
        (f32::from(bg[1]) * (1.0 - a)).round() as u8,
        (f32::from(bg[2]) * (1.0 - a)).round() as u8,
    ];
    assert_rgb(&img, 101, 101, expected, "blended");
}

#[test]
fn xrgb_windows_are_opaque() {
    // Alpha byte 0, but the format says to ignore it.
    let green = solid(10, 10, 0x0000_ff00);
    let img = render(
        &[(1, image(&green, 10, 10, true))],
        &[quad(1, 100.0, 100.0, 40.0, 40.0)],
    );
    assert_rgb(&img, 110, 110, [0, 255, 0], "opaque green");
}

#[test]
fn flipped_180_flips_vertically() {
    // A 2x2 image: top-left red, the rest blue.
    let mut bytes = solid(2, 2, 0xff00_00ff);
    bytes[..4].copy_from_slice(&0xffff_0000_u32.to_le_bytes());
    let mut q = quad(1, 100.0, 100.0, 40.0, 40.0);
    q.transform = Transform::Flipped180;
    let img = render(&[(1, image(&bytes, 2, 2, false))], &[q]);
    assert_rgb(
        &img,
        105,
        135,
        [255, 0, 0],
        "bottom-left is the buffer's top-left",
    );
    assert_rgb(&img, 105, 105, [0, 0, 255], "top-left is blue");
}

#[test]
fn a_part_placed_below_is_hidden_by_its_opaque_parent() {
    let red = solid(10, 10, 0xffff_0000);
    let blue = solid(10, 10, 0xff00_00ff);
    // The child (blue) is drawn first, then the parent (red) over it.
    let img = render(
        &[
            (1, image(&red, 10, 10, false)),
            (2, image(&blue, 10, 10, false)),
        ],
        &[
            quad(2, 120.0, 120.0, 40.0, 40.0),
            quad(1, 100.0, 100.0, 40.0, 40.0),
        ],
    );
    assert_rgb(&img, 130, 130, [255, 0, 0], "overlap is the parent");
    assert_rgb(&img, 150, 150, [0, 0, 255], "the rest of the child shows");
}

#[test]
fn thin_lines_shrunk_to_25_percent_read_as_even_grey() {
    // One-pixel white lines every fourth column: a quarter of the pixels are white. Shrunk to
    // 25%, every screen pixel must show their average (about 64). Without mipmaps the samples
    // fall between the lines and come out black (moire); this was checked by disabling them.
    let n = 64;
    let bytes: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let white = (i % n) % 4 == 0;
            if white { 0xffff_ffff_u32 } else { 0xff00_0000 }.to_le_bytes()
        })
        .collect();
    let img = render(
        &[(1, image(&bytes, n, n, false))],
        &[quad(1, 100.0, 60.0, 16.0, 16.0)],
    );
    for y in 61..75 {
        for x in 101..115 {
            let [r, g, b] = rgb(&img, x, y);
            for c in [r, g, b] {
                assert!(
                    c.abs_diff(64) <= 12,
                    "({x}, {y}) is {c}: moire or blocky pixels"
                );
            }
        }
    }
}

#[test]
fn a_fullscreen_window_is_drawn_alone_on_black() {
    let red = solid(120, 80, 0xffff_0000);
    let img = {
        let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
        match render::offscreen_fullscreen_scene(
            &[(1, image(&red, 120, 80, false))],
            &[quad(1, 100.0, 60.0, 120.0, 80.0)],
            W,
            H,
        ) {
            Ok(rgba) => rgba,
            Err(err) => panic!("offscreen render failed: {err}"),
        }
    };
    let black = render::FULLSCREEN_BACKGROUND.map(|c| (c * 255.0).round() as u8);
    assert_eq!(black, [0, 0, 0]);
    for y in 0..H {
        for x in 0..W {
            let inside = (100..220).contains(&x) && (60..140).contains(&y);
            let expected = if inside { [255, 0, 0] } else { black };
            assert_rgb(
                &img,
                x,
                y,
                expected,
                if inside { "window" } else { "no grid" },
            );
        }
    }
}
