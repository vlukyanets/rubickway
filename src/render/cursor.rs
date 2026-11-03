//! Pointer images: built-in ASCII-art images, or the installed Xcursor theme (research S10).
//!
//! The built-in images are drawn at 12 px and shown doubled, at 24 px. Images are premultiplied
//! ARGB in little-endian bytes (B, G, R, A), like `wl_shm`'s `argb8888`, so the renderer
//! uploads and draws them as window quads.

use std::path::{Path, PathBuf};

use super::{ImageRef, Quad, Transform};

/// The pointer images RubickWay draws (the shell's cursor shapes map onto these).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorKind {
    Arrow,
    Grab,
    Move,
    /// Left-right double arrow.
    ResizeEw,
    /// Up-down double arrow.
    ResizeNs,
    /// Top-left to bottom-right.
    ResizeNwse,
    /// Top-right to bottom-left.
    ResizeNesw,
}

impl CursorKind {
    pub const ALL: [CursorKind; 7] = [
        CursorKind::Arrow,
        CursorKind::Grab,
        CursorKind::Move,
        CursorKind::ResizeEw,
        CursorKind::ResizeNs,
        CursorKind::ResizeNwse,
        CursorKind::ResizeNesw,
    ];

    /// Xcursor names to look for, most specific first.
    fn theme_names(self) -> &'static [&'static str] {
        match self {
            CursorKind::Arrow => &["default", "left_ptr"],
            CursorKind::Grab => &["grabbing", "closedhand", "dnd-move"],
            CursorKind::Move => &["move", "fleur", "all-scroll"],
            CursorKind::ResizeEw => &["ew-resize", "sb_h_double_arrow", "h_double_arrow"],
            CursorKind::ResizeNs => &["ns-resize", "sb_v_double_arrow", "v_double_arrow"],
            CursorKind::ResizeNwse => &["nwse-resize", "bd_double_arrow", "size_fdiag"],
            CursorKind::ResizeNesw => &["nesw-resize", "fd_double_arrow", "size_bdiag"],
        }
    }

    /// A texture key for the renderer, outside the range used for surfaces.
    pub fn texture_key(self) -> u64 {
        u64::MAX - self as u64
    }
}

/// One pointer image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorImage {
    pub width: u32,
    pub height: u32,
    /// The point of the image at the pointer position.
    pub hotspot: (u32, u32),
    /// Premultiplied ARGB, little-endian (B, G, R, A), `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl CursorImage {
    /// The image for the renderer's `upload`.
    pub fn image_ref(&self) -> ImageRef<'_> {
        ImageRef {
            bytes: &self.pixels,
            width: self.width,
            height: self.height,
            stride: self.width * 4,
            opaque: false,
        }
    }

    /// The quad drawing this image (uploaded under `kind`'s key) with its hotspot at `pos`,
    /// in physical pixels.
    pub fn quad(&self, kind: CursorKind, pos: (f32, f32)) -> Quad {
        Quad {
            key: kind.texture_key(),
            dst: [
                pos.0 - self.hotspot.0 as f32,
                pos.1 - self.hotspot.1 as f32,
                self.width as f32,
                self.height as f32,
            ],
            src: [0.0, 0.0, 1.0, 1.0],
            transform: Transform::Normal,
        }
    }
}

/// The size images are shown at.
pub const CURSOR_SIZE: u32 = 24;
/// The built-in art is drawn at half that size and doubled.
const ART: usize = 12;

const ARROW: [&str; ART] = [
    "#           ",
    "##          ",
    "#.#         ",
    "#..#        ",
    "#...#       ",
    "#....#      ",
    "#.....#     ",
    "#......#    ",
    "#....####   ",
    "#.##.#      ",
    "##  #.#     ",
    "     ##     ",
];

const GRAB: [&str; ART] = [
    "            ",
    "  ## ## ##  ",
    " #..#..#..# ",
    " #........##",
    "#..........#",
    "#..........#",
    "#..........#",
    " #........# ",
    " #........# ",
    "  #......#  ",
    "  ########  ",
    "            ",
];

const MOVE: [&str; ART] = [
    "     ##     ",
    "    #..#    ",
    "   ##..##   ",
    " ## #..# ## ",
    "#..##..##..#",
    "#..........#",
    "#..........#",
    "#..##..##..#",
    " ## #..# ## ",
    "   ##..##   ",
    "    #..#    ",
    "     ##     ",
];

const RESIZE_EW: [&str; ART] = [
    "            ",
    "            ",
    "   #    #   ",
    "  ##    ##  ",
    " #.######.# ",
    "#..........#",
    "#..........#",
    " #.######.# ",
    "  ##    ##  ",
    "   #    #   ",
    "            ",
    "            ",
];

const RESIZE_NWSE: [&str; ART] = [
    "######      ",
    "#....#      ",
    "#...#       ",
    "#....#      ",
    "#.#...#     ",
    "##  #..#    ",
    "    #..#  ##",
    "     #...#.#",
    "      #....#",
    "       #...#",
    "      #....#",
    "      ######",
];

/// A built-in image, always available.
pub fn builtin(kind: CursorKind) -> CursorImage {
    let grid: Vec<Vec<u8>> = match kind {
        CursorKind::Arrow => rows(&ARROW),
        CursorKind::Grab => rows(&GRAB),
        CursorKind::Move => rows(&MOVE),
        CursorKind::ResizeEw => rows(&RESIZE_EW),
        CursorKind::ResizeNs => transpose(&rows(&RESIZE_EW)),
        CursorKind::ResizeNwse => rows(&RESIZE_NWSE),
        CursorKind::ResizeNesw => rows(&RESIZE_NWSE)
            .into_iter()
            .map(|mut r| {
                r.reverse();
                r
            })
            .collect(),
    };
    let hotspot = match kind {
        CursorKind::Arrow => (0, 0),
        _ => (CURSOR_SIZE / 2, CURSOR_SIZE / 2),
    };
    let mut pixels = Vec::with_capacity((CURSOR_SIZE * CURSOR_SIZE * 4) as usize);
    for y in 0..CURSOR_SIZE as usize {
        for x in 0..CURSOR_SIZE as usize {
            let c = grid
                .get(y / 2)
                .and_then(|r| r.get(x / 2))
                .copied()
                .unwrap_or(b' ');
            pixels.extend_from_slice(match c {
                b'#' => &[0, 0, 0, 255],
                b'.' => &[255, 255, 255, 255],
                _ => &[0, 0, 0, 0],
            });
        }
    }
    CursorImage {
        width: CURSOR_SIZE,
        height: CURSOR_SIZE,
        hotspot,
        pixels,
    }
}

fn rows(art: &[&str; ART]) -> Vec<Vec<u8>> {
    art.iter().map(|r| r.as_bytes().to_vec()).collect()
}

fn transpose(grid: &[Vec<u8>]) -> Vec<Vec<u8>> {
    (0..ART)
        .map(|x| {
            (0..ART)
                .map(|y| grid.get(y).and_then(|r| r.get(x)).copied().unwrap_or(b' '))
                .collect()
        })
        .collect()
}

/// The pointer images: the installed `default` cursor theme where it has an image, the
/// built-in ones otherwise. Reads files, so it runs once at startup (Principle VII).
pub fn load(icon_dirs: &[PathBuf]) -> Vec<(CursorKind, CursorImage)> {
    CursorKind::ALL
        .iter()
        .map(|&kind| {
            let image = theme_image(icon_dirs, "default", kind).unwrap_or_else(|| builtin(kind));
            (kind, image)
        })
        .collect()
}

/// The standard icon directories: `~/.local/share/icons`, `~/.icons`, `/usr/share/icons`,
/// `/usr/share/pixmaps`.
pub fn icon_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".local/share/icons"));
        dirs.push(home.join(".icons"));
    }
    dirs.push(PathBuf::from("/usr/share/icons"));
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    dirs
}

/// The image for `kind` from `theme` or the themes it inherits from (at most a few levels).
pub fn theme_image(icon_dirs: &[PathBuf], theme: &str, kind: CursorKind) -> Option<CursorImage> {
    let mut themes = vec![theme.to_string()];
    let mut i = 0;
    while let Some(name) = themes.get(i).cloned() {
        for dir in icon_dirs {
            let base = dir.join(&name);
            for file in kind.theme_names() {
                if let Ok(bytes) = std::fs::read(base.join("cursors").join(file))
                    && let Some(image) = parse_xcursor(&bytes, CURSOR_SIZE)
                {
                    return Some(image);
                }
            }
            if let Ok(index) = std::fs::read_to_string(base.join("index.theme")) {
                for parent in inherits(&index) {
                    if themes.len() < 8 && !themes.contains(&parent) {
                        themes.push(parent);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// The `Inherits=` list of an `index.theme`.
fn inherits(index: &str) -> Vec<String> {
    index
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Inherits="))
        .flat_map(|v| v.split([',', ';']))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

const XCURSOR_IMAGE: u32 = 0xfffd_0002;
/// Images larger than this are not used (no theme ships them; a broken file could claim more).
const XCURSOR_MAX_SIDE: u32 = 256;

/// The image of an Xcursor file whose nominal size is closest to `size`. Every length and
/// offset is checked, so a broken file gives `None`, never a panic.
pub fn parse_xcursor(bytes: &[u8], size: u32) -> Option<CursorImage> {
    let u32_at = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.get(..4) != Some(b"Xcur") {
        return None;
    }
    let header = u32_at(4)? as usize;
    let ntoc = u32_at(12)? as usize;
    let mut best: Option<(u32, usize)> = None;
    for i in 0..ntoc.min(1024) {
        let entry = header.checked_add(i.checked_mul(12)?)?;
        if u32_at(entry)? != XCURSOR_IMAGE {
            continue;
        }
        let nominal = u32_at(entry + 4)?;
        let position = u32_at(entry + 8)? as usize;
        let better = best.is_none_or(|(n, _)| nominal.abs_diff(size) < n.abs_diff(size));
        if better {
            best = Some((nominal, position));
        }
    }
    let (_, at) = best?;
    if u32_at(at.checked_add(4)?)? != XCURSOR_IMAGE {
        return None;
    }
    let chunk_header = u32_at(at)? as usize;
    let width = u32_at(at + 16)?;
    let height = u32_at(at + 20)?;
    let hotspot = (u32_at(at + 24)?, u32_at(at + 28)?);
    if width == 0 || height == 0 || width > XCURSOR_MAX_SIDE || height > XCURSOR_MAX_SIDE {
        return None;
    }
    let len = (width as usize) * (height as usize) * 4;
    let start = at.checked_add(chunk_header)?;
    let pixels = bytes.get(start..start.checked_add(len)?)?.to_vec();
    Some(CursorImage {
        width,
        height,
        hotspot: (hotspot.0.min(width - 1), hotspot.1.min(height - 1)),
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &CursorImage, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * img.width + x) * 4) as usize;
        [
            img.pixels[at],
            img.pixels[at + 1],
            img.pixels[at + 2],
            img.pixels[at + 3],
        ]
    }

    #[test]
    fn every_kind_has_a_24_px_image_with_its_hotspot_inside() {
        for kind in CursorKind::ALL {
            let img = builtin(kind);
            assert_eq!((img.width, img.height), (24, 24), "{kind:?}");
            assert_eq!(img.pixels.len(), 24 * 24 * 4, "{kind:?}");
            assert!(img.hotspot.0 < 24 && img.hotspot.1 < 24, "{kind:?}");
            assert!(
                img.pixels.chunks(4).any(|p| p[3] == 255),
                "{kind:?} draws something"
            );
        }
    }

    #[test]
    fn the_art_uses_only_outline_fill_and_transparent() {
        for art in [&ARROW, &GRAB, &MOVE, &RESIZE_EW, &RESIZE_NWSE] {
            for row in art.iter() {
                assert_eq!(row.len(), ART, "{row:?}");
                assert!(
                    row.bytes().all(|b| matches!(b, b' ' | b'#' | b'.')),
                    "{row:?}"
                );
            }
        }
    }

    #[test]
    fn colours_are_premultiplied_and_doubled() {
        let img = builtin(CursorKind::Arrow);
        // ARROW[0][0] is '#': the top-left 2x2 block is opaque black.
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(pixel(&img, x, y), [0, 0, 0, 255]);
        }
        // ARROW[2][1] is '.': opaque white at (2..4, 4..6).
        assert_eq!(pixel(&img, 2, 4), [255, 255, 255, 255]);
        // ARROW[0][11] is ' ': transparent.
        assert_eq!(pixel(&img, 23, 0), [0, 0, 0, 0]);
        assert_eq!(img.hotspot, (0, 0));
    }

    #[test]
    fn mirrored_and_transposed_images_differ_from_their_source() {
        assert_ne!(builtin(CursorKind::ResizeNs), builtin(CursorKind::ResizeEw));
        assert_ne!(
            builtin(CursorKind::ResizeNesw),
            builtin(CursorKind::ResizeNwse)
        );
    }

    #[test]
    fn no_theme_means_builtin_images() {
        let dir = std::env::temp_dir().join("rubickway-no-such-icons");
        assert_eq!(
            theme_image(std::slice::from_ref(&dir), "default", CursorKind::Arrow),
            None
        );
        let images = load(&[dir]);
        assert_eq!(images.len(), CursorKind::ALL.len());
        assert_eq!(images[0].1, builtin(CursorKind::Arrow));
    }

    /// A minimal Xcursor file with one `side` px image of `argb` and the given hotspot.
    fn xcursor(nominal: u32, side: u32, hotspot: (u32, u32), argb: u32) -> Vec<u8> {
        let le = |v: u32| v.to_le_bytes();
        let mut f = Vec::new();
        f.extend(b"Xcur");
        f.extend(le(16)); // header size
        f.extend(le(0x1_0000)); // version
        f.extend(le(1)); // ntoc
        f.extend(le(XCURSOR_IMAGE));
        f.extend(le(nominal));
        f.extend(le(28)); // image chunk position
        f.extend(le(36)); // chunk header size
        f.extend(le(XCURSOR_IMAGE));
        f.extend(le(nominal));
        f.extend(le(1)); // version
        f.extend(le(side));
        f.extend(le(side));
        f.extend(le(hotspot.0));
        f.extend(le(hotspot.1));
        f.extend(le(0)); // delay
        for _ in 0..side * side {
            f.extend(le(argb));
        }
        f
    }

    #[test]
    fn an_xcursor_image_is_parsed_with_its_hotspot() {
        let img = parse_xcursor(&xcursor(24, 24, (3, 4), 0xff11_2233), 24).expect("parsed");
        assert_eq!((img.width, img.height, img.hotspot), (24, 24, (3, 4)));
        assert_eq!(&img.pixels[..4], &[0x33, 0x22, 0x11, 0xff]);
    }

    #[test]
    fn broken_xcursor_files_give_none() {
        let good = xcursor(24, 24, (0, 0), 0xffff_ffff);
        assert!(
            parse_xcursor(&good[..good.len() - 1], 24).is_none(),
            "truncated"
        );
        assert!(parse_xcursor(b"Xcu", 24).is_none());
        assert!(
            parse_xcursor(&xcursor(24, 300, (0, 0), 0), 24).is_none(),
            "too large"
        );
        let mut bad = good.clone();
        bad[0] = b'Y';
        assert!(parse_xcursor(&bad, 24).is_none());
    }

    #[test]
    fn a_theme_on_disk_is_found_through_inherits() {
        let root = std::env::temp_dir().join(format!("rubickway-icons-{}", std::process::id()));
        let parent = root.join("Adwaita/cursors");
        std::fs::create_dir_all(&parent).expect("dirs");
        std::fs::create_dir_all(root.join("default")).expect("dirs");
        std::fs::write(
            root.join("default/index.theme"),
            "[Icon Theme]\nInherits=Adwaita\n",
        )
        .expect("index");
        std::fs::write(
            parent.join("left_ptr"),
            xcursor(24, 24, (1, 2), 0xff00_0000),
        )
        .expect("cursor");
        let img =
            theme_image(std::slice::from_ref(&root), "default", CursorKind::Arrow).expect("found");
        assert_eq!(img.hotspot, (1, 2));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
