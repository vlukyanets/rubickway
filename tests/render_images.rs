//! Rendering tests for GPU images: offscreen on llvmpipe, read back, checked against the same
//! picture drawn as a shared-memory window (FR-004). External textures here are made from ordinary
//! textures, so no dma-buf and no device is needed (research G11); real imports are in the
//! quickstart.
#![expect(
    clippy::expect_used,
    reason = "the render helpers fail loudly; clippy.toml only exempts #[test] functions"
)]

use std::sync::Mutex;

use rubickway::canvas::{Camera, Point, Size};
use rubickway::render::egl::Egl;
use rubickway::render::{self, GlesRenderer, GridParams, ImageRef, Quad, RenderError, Transform};

static GPU: Mutex<()> = Mutex::new(());

const W: u32 = 64;
const H: u32 = 64;
const SURFACE: u64 = 7;
const IMAGE: u64 = 70;

/// Pixels as a GPU image holds them, row 0 first: `[r, g, b, a]`.
type Rgba = [u8; 4];

fn grid() -> GridParams {
    let camera = Camera {
        center: Point::new(10_000.5, 10_000.5),
        zoom: 1.0,
    };
    GridParams::new(&camera, Size::new(f64::from(W), f64::from(H)), 1.0)
}

fn quad(key: u64, w: f32, h: f32) -> Quad {
    Quad {
        key,
        dst: [0.0, 0.0, w, h],
        src: [0.0, 0.0, 1.0, 1.0],
        transform: Transform::Normal,
    }
}

/// The GPU image: an ordinary texture holding `pixels`, bound to an EGLImage.
fn add_image(
    egl: &Egl,
    renderer: &mut GlesRenderer,
    key: u64,
    pixels: &[Rgba],
    width: u32,
    height: u32,
) -> Result<(), RenderError> {
    let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
    renderer.add_image_from_pixels(egl, key, &bytes, (width, height), true)
}

/// Draws `pixels` as a GPU image shown by a surface.
fn as_image(pixels: &[Rgba], width: u32, height: u32, y_invert: bool, q: Quad) -> Vec<u8> {
    as_image_checking(pixels, width, height, y_invert, q, |_| {})
}

fn as_image_checking(
    pixels: &[Rgba],
    width: u32,
    height: u32,
    y_invert: bool,
    q: Quad,
    check: impl FnOnce(&mut GlesRenderer),
) -> Vec<u8> {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, pixels, width, height)?;
        renderer.show_image(SURFACE, IMAGE, y_invert);
        Ok(())
    };
    render::offscreen_with_images(setup, &grid(), &[q], W, H, check).expect("render")
}

/// Draws the same `pixels` as a shared-memory window: bytes B, G, R, A.
fn as_shm(pixels: &[Rgba], width: u32, height: u32, q: Quad) -> Vec<u8> {
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|[r, g, b, a]| [*b, *g, *r, *a])
        .collect();
    let image = ImageRef {
        bytes: &bytes,
        width,
        height,
        stride: width * 4,
        opaque: false,
    };
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    render::offscreen_scene(&grid(), &[(SURFACE, image)], &[q], W, H).expect("render")
}

fn pixel(rgba: &[u8], x: u32, y: u32) -> Rgba {
    let at = ((y * W + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
}

/// Whether two pictures are the same within `tolerance` per channel.
fn assert_same(a: &[u8], b: &[u8], tolerance: u8) {
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let at = i / 4;
        assert!(
            x.abs_diff(*y) <= tolerance,
            "pixel ({}, {}) channel {}: {x} against {y}",
            at as u32 % W,
            at as u32 / W,
            i % 4
        );
    }
}

/// Two columns: `left` and `right`, over `size x size`.
fn columns(left: Rgba, right: Rgba, size: u32) -> Vec<Rgba> {
    (0..size * size)
        .map(|i| if i % size < size / 2 { left } else { right })
        .collect()
}

/// Two rows: `top` and `bottom`, over `size x size`.
fn rows(top: Rgba, bottom: Rgba, size: u32) -> Vec<Rgba> {
    (0..size * size)
        .map(|i| if i / size < size / 2 { top } else { bottom })
        .collect()
}

const RED: Rgba = [255, 0, 0, 255];
const GREEN: Rgba = [0, 255, 0, 255];
const BLUE: Rgba = [0, 0, 255, 255];
/// Premultiplied half-transparent white.
const HALF: Rgba = [128, 128, 128, 128];

#[test]
fn colours_are_those_of_a_shared_memory_window() {
    let pixels = columns(RED, BLUE, 32);
    let q = quad(SURFACE, 32.0, 32.0);
    let image = as_image(&pixels, 32, 32, false, q);
    assert_eq!(pixel(&image, 2, 2), RED);
    assert_eq!(pixel(&image, 30, 2), BLUE);
    assert_same(&image, &as_shm(&pixels, 32, 32, q), 1);
}

#[test]
fn transparency_is_that_of_a_shared_memory_window() {
    let pixels = columns(HALF, GREEN, 32);
    let q = quad(SURFACE, 32.0, 32.0);
    assert_same(
        &as_image(&pixels, 32, 32, false, q),
        &as_shm(&pixels, 32, 32, q),
        1,
    );
}

#[test]
fn a_format_without_alpha_is_opaque_at_any_zoom() {
    // Alpha 0 in the data: if it were read, the window would not show at all.
    let bytes: Vec<u8> = (0..256 * 256).flat_map(|_| [255u8, 0, 0, 0]).collect();
    for (side, copies) in [(256.0, 0), (32.0, 1)] {
        let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
        let q = quad(SURFACE, side, side);
        let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
            renderer.add_image_from_pixels(egl, IMAGE, &bytes, (256, 256), false)?;
            renderer.show_image(SURFACE, IMAGE, false);
            Ok(())
        };
        let rgba = render::offscreen_with_images(setup, &grid(), &[q], W, H, |renderer| {
            assert_eq!(renderer.mip_copies(), copies);
        })
        .expect("render");
        assert_eq!(pixel(&rgba, 16, 16), RED, "drawn at {side}");
    }
}

#[test]
fn an_image_marked_flipped_shows_the_right_way_up() {
    let pixels = rows(RED, GREEN, 32);
    let q = quad(SURFACE, 32.0, 32.0);
    let straight = as_image(&pixels, 32, 32, false, q);
    assert_eq!(pixel(&straight, 16, 2), RED);
    assert_eq!(pixel(&straight, 16, 30), GREEN);
    let flipped = as_image(&pixels, 32, 32, true, q);
    assert_eq!(pixel(&flipped, 16, 2), GREEN);
    assert_eq!(pixel(&flipped, 16, 30), RED);
}

#[test]
fn every_buffer_transform_is_that_of_a_shared_memory_window() {
    // A picture with no symmetry: a different colour in three corners.
    let size = 32u32;
    let pixels: Vec<Rgba> = (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            match (x < size / 2, y < size / 2) {
                (true, true) => RED,
                (false, true) => GREEN,
                (true, false) => BLUE,
                (false, false) => [255, 255, 255, 255],
            }
        })
        .collect();
    for transform in [
        Transform::Normal,
        Transform::Rotate90,
        Transform::Rotate180,
        Transform::Rotate270,
        Transform::Flipped,
        Transform::Flipped90,
        Transform::Flipped180,
        Transform::Flipped270,
    ] {
        let q = Quad {
            transform,
            ..quad(SURFACE, 32.0, 32.0)
        };
        assert_same(
            &as_image(&pixels, size, size, false, q),
            &as_shm(&pixels, size, size, q),
            1,
        );
    }
}

/// A checkerboard of one-pixel squares: aliases badly unless filtered with mipmaps.
fn checker(size: u32) -> Vec<Rgba> {
    (0..size * size)
        .map(|i| {
            if (i % size + i / size).is_multiple_of(2) {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
        .collect()
}

#[test]
fn zoomed_out_it_does_not_alias_like_a_shared_memory_window() {
    let pixels = checker(256);
    let q = quad(SURFACE, 32.0, 32.0);
    let image = as_image_checking(&pixels, 256, 256, false, q, |renderer| {
        assert_eq!(renderer.mip_copies(), 1, "zoomed out: a mipmap copy");
    });
    // The checkerboard averages to mid grey, as on a shared-memory window.
    let centre = pixel(&image, 16, 16);
    assert!((100..=156).contains(&centre[0]), "{centre:?}");
    assert_same(&image, &as_shm(&pixels, 256, 256, q), 8);
}

#[test]
fn a_flipped_image_zoomed_out_stays_the_right_way_up() {
    let pixels = rows(RED, GREEN, 256);
    let q = quad(SURFACE, 32.0, 32.0);
    let straight = as_image(&pixels, 256, 256, false, q);
    assert_eq!(pixel(&straight, 16, 2), RED);
    assert_eq!(pixel(&straight, 16, 30), GREEN);
    let flipped = as_image(&pixels, 256, 256, true, q);
    assert_eq!(pixel(&flipped, 16, 2), GREEN);
    assert_eq!(pixel(&flipped, 16, 30), RED);
}

#[test]
fn at_full_size_no_copy_is_made() {
    let pixels = checker(32);
    let q = quad(SURFACE, 32.0, 32.0);
    as_image_checking(&pixels, 32, 32, false, q, |renderer| {
        assert_eq!(renderer.mip_copies(), 0);
    });
}

#[test]
fn the_copy_goes_with_the_next_image() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let first = checker(64);
    let second = columns(RED, BLUE, 64);
    let q = quad(SURFACE, 32.0, 32.0);
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &first, 64, 64)?;
        add_image(egl, renderer, IMAGE + 1, &second, 64, 64)?;
        renderer.show_image(SURFACE, IMAGE, false);
        renderer.prepare_frame(&[q], None)?;
        assert_eq!(renderer.mip_copies(), 1);
        renderer.show_image(SURFACE, IMAGE + 1, false);
        assert_eq!(renderer.mip_copies(), 0);
        Ok(())
    };
    let rgba = render::offscreen_with_images(setup, &grid(), &[q], W, H, |_| {}).expect("render");
    assert_eq!(pixel(&rgba, 2, 2), RED, "the new image is drawn");
}

/// Creating a texture of a large window's size takes the event loop hundreds of milliseconds, so
/// each new image of a window must not do it (Principle VII, research G4).
#[test]
fn the_next_image_of_the_same_size_reuses_the_texture_with_all_its_detail_new() {
    let first = columns(RED, BLUE, 256);
    let second = checker(256);
    let q = quad(SURFACE, 64.0, 64.0);
    let expected = as_shm(&second, 256, 256, q);
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &first, 256, 256)?;
        add_image(egl, renderer, IMAGE + 1, &second, 256, 256)?;
        renderer.show_image(SURFACE, IMAGE, false);
        renderer.prepare_frame(&[q], None)?;
        renderer.show_image(SURFACE, IMAGE + 1, false);
        Ok(())
    };
    let rgba = render::offscreen_with_images(setup, &grid(), &[q], W, H, |renderer| {
        assert_eq!(renderer.mip_copies(), 1);
        assert_eq!(renderer.copy_textures_made(), 1);
    })
    .expect("render");
    assert_same(&rgba, &expected, 8);
}

#[test]
fn an_image_of_another_size_gets_its_own_texture() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let first = checker(64);
    let second = columns(RED, BLUE, 128);
    let q = quad(SURFACE, 32.0, 32.0);
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &first, 64, 64)?;
        add_image(egl, renderer, IMAGE + 1, &second, 128, 128)?;
        renderer.show_image(SURFACE, IMAGE, false);
        renderer.prepare_frame(&[q], None)?;
        renderer.show_image(SURFACE, IMAGE + 1, false);
        Ok(())
    };
    let rgba = render::offscreen_with_images(setup, &grid(), &[q], W, H, |renderer| {
        assert_eq!(renderer.copy_textures_made(), 2);
    })
    .expect("render");
    assert_eq!(pixel(&rgba, 2, 2), RED);
}

#[test]
fn a_texture_nobody_needs_any_more_is_freed_within_some_frames() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let pixels = checker(64);
    let small = quad(SURFACE, 32.0, 32.0);
    let full = quad(SURFACE, 64.0, 64.0);
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &pixels, 64, 64)?;
        add_image(egl, renderer, IMAGE + 1, &pixels, 64, 64)?;
        renderer.show_image(SURFACE, IMAGE, false);
        renderer.prepare_frame(&[small], None)?;
        // The next image is drawn at full size: it needs no copy.
        renderer.show_image(SURFACE, IMAGE + 1, false);
        renderer.prepare_frame(&[full], None)?;
        assert_eq!(renderer.spare_copy_textures(), 1, "kept for a moment");
        for _ in 0..30 {
            renderer.prepare_frame(&[full], None)?;
        }
        assert_eq!(renderer.spare_copy_textures(), 0, "not for ever");
        Ok(())
    };
    render::offscreen_with_images(setup, &grid(), &[full], W, H, |_| {}).expect("render");
}

#[test]
fn the_texture_of_a_surface_that_went_away_is_freed_too() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let pixels = checker(64);
    let small = quad(SURFACE, 32.0, 32.0);
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &pixels, 64, 64)?;
        renderer.show_image(SURFACE, IMAGE, false);
        renderer.prepare_frame(&[small], None)?;
        renderer.remove(SURFACE);
        assert_eq!(renderer.spare_copy_textures(), 1);
        for _ in 0..30 {
            renderer.prepare_frame(&[], None)?;
        }
        assert_eq!(renderer.spare_copy_textures(), 0);
        Ok(())
    };
    render::offscreen_with_images(setup, &grid(), &[], W, H, |_| {}).expect("render");
}

// --- Distant windows (FR-017) ---------------------------------------------------------------

/// Runs `steps` with image `pixels` shown on the surface, then draws `last` at a detail of 1.
fn with_detail(
    pixels: &[Rgba],
    size: u32,
    steps: impl FnOnce(&mut GlesRenderer) -> Result<(), RenderError>,
    last: Quad,
) -> Vec<u8> {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, pixels, size, size)?;
        renderer.show_image(SURFACE, IMAGE, false);
        steps(renderer)
    };
    render::offscreen_with_detail(Some(1.0), setup, &grid(), &[last], W, H, |_| {}).expect("render")
}

#[test]
fn without_a_detail_the_copy_has_the_size_of_the_image() {
    let q = quad(SURFACE, 32.0, 32.0);
    with_detail(
        &columns(RED, BLUE, 256),
        256,
        |renderer| {
            renderer.prepare_frame(&[q], None)?;
            assert_eq!(renderer.mip_copy_size(SURFACE), Some((256, 256)));
            Ok(())
        },
        q,
    );
}

#[test]
fn with_a_detail_the_copy_follows_the_drawn_size_and_keeps_the_colours() {
    let q = quad(SURFACE, 32.0, 32.0);
    let rgba = with_detail(
        &columns(RED, BLUE, 256),
        256,
        |renderer| {
            renderer.prepare_frame(&[q], Some(1.0))?;
            assert_eq!(renderer.mip_copy_size(SURFACE), Some((32, 32)));
            Ok(())
        },
        q,
    );
    assert_eq!(pixel(&rgba, 2, 2), RED);
    assert_eq!(pixel(&rgba, 30, 2), BLUE);
}

#[test]
fn a_flipped_image_stays_the_right_way_up_in_a_smaller_copy() {
    let q = quad(SURFACE, 32.0, 32.0);
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let pixels = rows(RED, GREEN, 256);
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &pixels, 256, 256)?;
        renderer.show_image(SURFACE, IMAGE, true);
        Ok(())
    };
    let rgba = render::offscreen_with_detail(Some(1.0), setup, &grid(), &[q], W, H, |renderer| {
        assert_eq!(renderer.mip_copy_size(SURFACE), Some((32, 32)));
    })
    .expect("render");
    assert_eq!(pixel(&rgba, 16, 2), GREEN);
    assert_eq!(pixel(&rgba, 16, 30), RED);
}

#[test]
fn zooming_in_past_what_the_copy_holds_makes_a_larger_one() {
    let far = quad(SURFACE, 16.0, 16.0);
    let nearer = quad(SURFACE, 32.0, 32.0);
    with_detail(
        &checker(64),
        64,
        |renderer| {
            renderer.prepare_frame(&[far], Some(1.0))?;
            assert_eq!(renderer.mip_copy_size(SURFACE), Some((16, 16)));
            renderer.prepare_frame(&[nearer], Some(1.0))?;
            assert_eq!(renderer.mip_copy_size(SURFACE), Some((32, 32)));
            // Going back out keeps what it has: two displays at different zooms would otherwise
            // make a copy every frame.
            renderer.prepare_frame(&[far], Some(1.0))?;
            assert_eq!(renderer.mip_copy_size(SURFACE), Some((32, 32)));
            Ok(())
        },
        far,
    );
}

#[test]
fn at_full_size_a_smaller_copy_is_not_what_is_drawn() {
    let far = quad(SURFACE, 16.0, 16.0);
    let full = quad(SURFACE, 64.0, 64.0);
    let pixels = checker(64);
    let rgba = with_detail(
        &pixels,
        64,
        |renderer| {
            renderer.prepare_frame(&[far], Some(1.0))?;
            Ok(())
        },
        full,
    );
    assert_eq!(
        pixel(&rgba, 10, 10),
        [255, 255, 255, 255],
        "every square sharp"
    );
    assert_eq!(pixel(&rgba, 11, 10), [0, 0, 0, 255]);
}

#[test]
fn an_image_stays_while_a_surface_shows_it() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let pixels = columns(RED, BLUE, 32);
    let q = quad(SURFACE, 32.0, 32.0);
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &pixels, 32, 32)?;
        renderer.show_image(SURFACE, IMAGE, false);
        // Its `wl_buffer` is destroyed while shown: the surface keeps its picture.
        renderer.remove_image(IMAGE);
        Ok(())
    };
    let rgba = render::offscreen_with_images(setup, &grid(), &[q], W, H, |_| {}).expect("render");
    assert_eq!(pixel(&rgba, 2, 2), RED);
}

#[test]
fn shared_memory_and_an_image_can_take_turns_on_one_surface() {
    let _guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let pixels = columns(RED, BLUE, 32);
    let q = quad(SURFACE, 32.0, 32.0);
    let green: Vec<u8> = (0..32 * 32).flat_map(|_| [0u8, 255, 0, 255]).collect();
    let setup = |egl: &Egl, renderer: &mut GlesRenderer| {
        add_image(egl, renderer, IMAGE, &pixels, 32, 32)?;
        renderer.show_image(SURFACE, IMAGE, false);
        renderer.upload(
            SURFACE,
            ImageRef {
                bytes: &green,
                width: 32,
                height: 32,
                stride: 128,
                opaque: false,
            },
        )?;
        Ok(())
    };
    let rgba = render::offscreen_with_images(setup, &grid(), &[q], W, H, |_| {}).expect("render");
    assert_eq!(pixel(&rgba, 2, 2), GREEN, "the last one wins");
}
