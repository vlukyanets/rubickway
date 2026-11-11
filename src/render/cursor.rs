//! Pointer images: built-in ASCII-art images, or the installed Xcursor theme (research S10).
//!
//! The built-in images are drawn at 12 px and shown doubled, at 24 px. Images are premultiplied
//! ARGB in little-endian bytes (B, G, R, A), like `wl_shm`'s `argb8888`, so the renderer
//! uploads and draws them as window quads.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{ImageRef, Quad, Transform};
use crate::shell::CursorName;

/// The pointer images RubickWay draws: one per shape applications can name, which RubickWay's
/// own gestures use too, and one for the window move gesture (cursor-shapes research C5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorKind {
    Shape(CursorName),
    /// RubickWay's window move gesture.
    WindowMove,
}

impl CursorKind {
    /// Every kind: the 36 shapes in protocol order, then the window move.
    pub const ALL: [CursorKind; 37] = {
        let mut all = [CursorKind::WindowMove; 37];
        let mut i = 0;
        while i < CursorName::ALL.len() {
            all[i] = CursorKind::Shape(CursorName::ALL[i]);
            i += 1;
        }
        all
    };

    /// The kind's place in [`CursorKind::ALL`].
    fn index(self) -> usize {
        match self {
            CursorKind::Shape(name) => name as usize,
            CursorKind::WindowMove => CursorName::ALL.len(),
        }
    }

    /// Xcursor names to look for, most specific first: the CSS name, older names of the same
    /// meaning, then the image of the same axis (cursor-shapes research C5).
    fn theme_names(self) -> &'static [&'static str] {
        use CursorName as N;
        let CursorKind::Shape(name) = self else {
            return &["move", "fleur", "all-scroll"];
        };
        match name {
            N::Default => &["default", "left_ptr"],
            N::ContextMenu => &["context-menu"],
            N::Help => &["help", "question_arrow", "whats_this", "left_ptr_help"],
            N::Pointer => &["pointer", "hand2", "pointing_hand", "hand1"],
            N::Progress => &["progress", "left_ptr_watch", "half-busy"],
            N::Wait => &["wait", "watch"],
            N::Cell => &["cell", "plus"],
            N::Crosshair => &["crosshair", "cross", "tcross"],
            N::Text => &["text", "xterm", "ibeam"],
            N::VerticalText => &["vertical-text"],
            N::Alias => &["alias", "dnd-link", "link"],
            N::Copy => &["copy", "dnd-copy"],
            N::Move => &["move", "dnd-move"],
            N::DndAsk => &["dnd-ask"],
            N::NoDrop => &["no-drop", "dnd-no-drop", "not-allowed", "crossed_circle"],
            N::NotAllowed => &["not-allowed", "crossed_circle", "forbidden"],
            N::Grab => &["grab", "openhand"],
            N::Grabbing => &["grabbing", "closedhand", "dnd-move"],
            N::EResize => &["e-resize", "right_side", "ew-resize", "sb_h_double_arrow"],
            N::WResize => &["w-resize", "left_side", "ew-resize", "sb_h_double_arrow"],
            N::NResize => &["n-resize", "top_side", "ns-resize", "sb_v_double_arrow"],
            N::SResize => &["s-resize", "bottom_side", "ns-resize", "sb_v_double_arrow"],
            N::NeResize => &[
                "ne-resize",
                "top_right_corner",
                "nesw-resize",
                "fd_double_arrow",
            ],
            N::SwResize => &[
                "sw-resize",
                "bottom_left_corner",
                "nesw-resize",
                "fd_double_arrow",
            ],
            N::NwResize => &[
                "nw-resize",
                "top_left_corner",
                "nwse-resize",
                "bd_double_arrow",
            ],
            N::SeResize => &[
                "se-resize",
                "bottom_right_corner",
                "nwse-resize",
                "bd_double_arrow",
            ],
            N::EwResize => &["ew-resize", "sb_h_double_arrow", "h_double_arrow"],
            N::NsResize => &["ns-resize", "sb_v_double_arrow", "v_double_arrow"],
            N::NeswResize => &["nesw-resize", "fd_double_arrow", "size_bdiag"],
            N::NwseResize => &["nwse-resize", "bd_double_arrow", "size_fdiag"],
            N::ColResize => &["col-resize", "split_h", "ew-resize", "sb_h_double_arrow"],
            N::RowResize => &["row-resize", "split_v", "ns-resize", "sb_v_double_arrow"],
            N::AllScroll => &["all-scroll", "fleur"],
            N::AllResize => &["all-resize", "all-scroll", "fleur"],
            N::ZoomIn => &["zoom-in"],
            N::ZoomOut => &["zoom-out"],
        }
    }

    /// A texture key for the renderer, outside the range used for surfaces.
    pub fn texture_key(self) -> u64 {
        u64::MAX - self.index() as u64
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

const POINTER: [&str; ART] = [
    "    ##      ",
    "   #..#     ",
    "   #..#     ",
    "   #..###   ",
    "   #..#..## ",
    " ###..#..#.#",
    "#..#.......#",
    "#..........#",
    " #.........#",
    "  #.......# ",
    "   #.....#  ",
    "   #######  ",
];

const HELP: [&str; ART] = [
    "#           ",
    "##          ",
    "#.#         ",
    "#..#   #### ",
    "#...# #....#",
    "#.##  #.##.#",
    "##     # #.#",
    "        #..#",
    "        #.# ",
    "        ### ",
    "        #.# ",
    "        ### ",
];

const PROGRESS: [&str; ART] = [
    "#           ",
    "##          ",
    "#.#         ",
    "#..#        ",
    "#...#       ",
    "#....#      ",
    "#.####      ",
    "##    ######",
    "      #....#",
    "       #..# ",
    "      #....#",
    "      ######",
];

const WAIT: [&str; ART] = [
    "############",
    "#..........#",
    " #........# ",
    "  #......#  ",
    "   #....#   ",
    "    #..#    ",
    "    #..#    ",
    "   #....#   ",
    "  #......#  ",
    " #...##...# ",
    "#..######..#",
    "############",
];

const CELL: [&str; ART] = [
    "   ######   ",
    "   #....#   ",
    "   #....#   ",
    "####....####",
    "#..........#",
    "#..........#",
    "#..........#",
    "#..........#",
    "####....####",
    "   #....#   ",
    "   #....#   ",
    "   ######   ",
];

const CROSSHAIR: [&str; ART] = [
    "    .##.    ",
    "    .##.    ",
    "    .##.    ",
    "    .##.    ",
    "....    ....",
    "####    ####",
    "####    ####",
    "....    ....",
    "    .##.    ",
    "    .##.    ",
    "    .##.    ",
    "    .##.    ",
];

const TEXT: [&str; ART] = [
    "  ###  ###  ",
    "  #..##..#  ",
    "  ###..###  ",
    "    #..#    ",
    "    #..#    ",
    "    #..#    ",
    "    #..#    ",
    "    #..#    ",
    "    #..#    ",
    "  ###..###  ",
    "  #..##..#  ",
    "  ###  ###  ",
];

const NOT_ALLOWED: [&str; ART] = [
    "   ######   ",
    "  ########  ",
    " ###....### ",
    "####.....###",
    "##.##.....##",
    "##..##....##",
    "##...##...##",
    "##....##..##",
    "###....#####",
    " ###....### ",
    "  ########  ",
    "   ######   ",
];

const OPEN_HAND: [&str; ART] = [
    "   ## ## ## ",
    "  #..#..#..#",
    "  #..#..#..#",
    "  #..#..#..#",
    " ##........#",
    "#..#.......#",
    "#...#......#",
    " #.........#",
    "  #........#",
    "   #......# ",
    "    #.....# ",
    "    ####### ",
];

const ZOOM_IN: [&str; ART] = [
    "  #####     ",
    " ##...##    ",
    "##..#..##   ",
    "#...#...#   ",
    "#.#####.#   ",
    "#...#...#   ",
    "##..#..##   ",
    " ##...###   ",
    "  ##### ##  ",
    "         ## ",
    "          ##",
    "           #",
];

/// Every drawing as written (some built-in images are these mirrored or transposed).
#[cfg(test)]
const ALL_ART: [&[&str; ART]; 15] = [
    &ARROW,
    &GRAB,
    &MOVE,
    &RESIZE_EW,
    &RESIZE_NWSE,
    &POINTER,
    &HELP,
    &PROGRESS,
    &WAIT,
    &CELL,
    &CROSSHAIR,
    &TEXT,
    &NOT_ALLOWED,
    &OPEN_HAND,
    &ZOOM_IN,
];

/// The built-in drawings (cursor-shapes FR-006, research C5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Art {
    Arrow,
    Pointer,
    Help,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    NotAllowed,
    Grab,
    Grabbing,
    AllDirections,
    ResizeEw,
    ResizeNs,
    ResizeNwse,
    ResizeNesw,
    ZoomIn,
    ZoomOut,
}

impl Art {
    #[cfg(test)]
    const ALL: [Art; 19] = [
        Art::Arrow,
        Art::Pointer,
        Art::Help,
        Art::Progress,
        Art::Wait,
        Art::Cell,
        Art::Crosshair,
        Art::Text,
        Art::VerticalText,
        Art::NotAllowed,
        Art::Grab,
        Art::Grabbing,
        Art::AllDirections,
        Art::ResizeEw,
        Art::ResizeNs,
        Art::ResizeNwse,
        Art::ResizeNesw,
        Art::ZoomIn,
        Art::ZoomOut,
    ];

    fn grid(self) -> Vec<Vec<u8>> {
        match self {
            Art::Arrow => rows(&ARROW),
            Art::Pointer => rows(&POINTER),
            Art::Help => rows(&HELP),
            Art::Progress => rows(&PROGRESS),
            Art::Wait => rows(&WAIT),
            Art::Cell => rows(&CELL),
            Art::Crosshair => rows(&CROSSHAIR),
            Art::Text => rows(&TEXT),
            Art::VerticalText => transpose(&rows(&TEXT)),
            Art::NotAllowed => rows(&NOT_ALLOWED),
            Art::Grab => rows(&OPEN_HAND),
            Art::Grabbing => rows(&GRAB),
            Art::AllDirections => rows(&MOVE),
            Art::ResizeEw => rows(&RESIZE_EW),
            Art::ResizeNs => transpose(&rows(&RESIZE_EW)),
            Art::ResizeNwse => rows(&RESIZE_NWSE),
            Art::ResizeNesw => mirror(&rows(&RESIZE_NWSE)),
            Art::ZoomIn => rows(&ZOOM_IN),
            // Zoom in without the vertical stroke of its plus.
            Art::ZoomOut => {
                let mut grid = rows(&ZOOM_IN);
                for (y, row) in grid.iter_mut().enumerate().take(7).skip(2) {
                    if y != 4
                        && let Some(c) = row.get_mut(4)
                    {
                        *c = b'.';
                    }
                }
                grid
            }
        }
    }

    /// The point at the pointer position, in shown pixels (FR-008).
    fn hotspot(self) -> (u32, u32) {
        match self {
            Art::Arrow | Art::Help | Art::Progress => (0, 0),
            // The fingertip.
            Art::Pointer => (9, 0),
            // The lens centre: cell (4, 4).
            Art::ZoomIn | Art::ZoomOut => (9, 9),
            _ => (CURSOR_SIZE / 2, CURSOR_SIZE / 2),
        }
    }
}

/// The drawing a kind is shown with when no theme has an image for it (FR-006, FR-007).
fn art_of(kind: CursorKind) -> Art {
    use CursorName as N;
    let CursorKind::Shape(name) = kind else {
        return Art::AllDirections;
    };
    match name {
        N::Default | N::ContextMenu | N::Alias | N::Copy | N::Move | N::DndAsk => Art::Arrow,
        N::Help => Art::Help,
        N::Pointer => Art::Pointer,
        N::Progress => Art::Progress,
        N::Wait => Art::Wait,
        N::Cell => Art::Cell,
        N::Crosshair => Art::Crosshair,
        N::Text => Art::Text,
        N::VerticalText => Art::VerticalText,
        N::NoDrop | N::NotAllowed => Art::NotAllowed,
        N::Grab => Art::Grab,
        N::Grabbing => Art::Grabbing,
        N::EResize | N::WResize | N::EwResize | N::ColResize => Art::ResizeEw,
        N::NResize | N::SResize | N::NsResize | N::RowResize => Art::ResizeNs,
        N::NeResize | N::SwResize | N::NeswResize => Art::ResizeNesw,
        N::NwResize | N::SeResize | N::NwseResize => Art::ResizeNwse,
        N::AllScroll | N::AllResize => Art::AllDirections,
        N::ZoomIn => Art::ZoomIn,
        N::ZoomOut => Art::ZoomOut,
    }
}

/// Whether a kind has a drawing of its own; the drag-and-drop shapes and the context menu show
/// the arrow, with a log line the first time (FR-007).
pub fn has_own_art(kind: CursorKind) -> bool {
    use CursorName as N;
    !matches!(
        kind,
        CursorKind::Shape(N::ContextMenu | N::Alias | N::Copy | N::Move | N::DndAsk)
    )
}

/// A built-in image, always available.
pub fn builtin(kind: CursorKind) -> CursorImage {
    draw(art_of(kind))
}

fn draw(art: Art) -> CursorImage {
    let grid = art.grid();
    let hotspot = art.hotspot();
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

fn mirror(grid: &[Vec<u8>]) -> Vec<Vec<u8>> {
    grid.iter()
        .map(|r| r.iter().rev().copied().collect())
        .collect()
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

/// Where a pointer image came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSource {
    Theme,
    Builtin,
}

/// One image per [`CursorKind`] (cursor-shapes data-model "PointerImages").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerImages {
    /// In the order of [`CursorKind::ALL`].
    entries: Vec<(CursorKind, CursorImage, ImageSource)>,
}

impl PointerImages {
    /// Every kind with its image.
    pub fn iter(&self) -> impl Iterator<Item = (CursorKind, &CursorImage)> {
        self.entries.iter().map(|(kind, image, _)| (*kind, image))
    }

    pub fn get(&self, kind: CursorKind) -> Option<&CursorImage> {
        self.entries.get(kind.index()).map(|(_, image, _)| image)
    }

    pub fn source(&self, kind: CursorKind) -> Option<ImageSource> {
        self.entries.get(kind.index()).map(|(_, _, source)| *source)
    }

    /// The kind is shown with the arrow because neither the theme nor RubickWay has an image
    /// for it (FR-007).
    pub fn arrow_stand_in(&self, kind: CursorKind) -> bool {
        !has_own_art(kind) && self.source(kind) == Some(ImageSource::Builtin)
    }
}

/// Whether a theme was used, or why not (cursor-shapes data-model "ThemeReport").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeUse {
    /// The theme of this name gave at least one image.
    Used(String),
    /// No theme of the name to use (`default` once a configured one is missing).
    NotInstalled,
    /// The theme is installed but neither it nor what it inherits has any image; `missing`: the
    /// inherited themes that are not installed.
    NoImages { name: String, missing: Vec<String> },
}

/// A theme file that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damaged {
    pub shape: &'static str,
    pub path: PathBuf,
    pub reason: XcursorError,
}

/// What [`load`] found, for the log (FR-012, contracts/cli.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeReport {
    pub theme: ThemeUse,
    /// The configured theme, when it is not installed.
    pub configured_missing: Option<String>,
    /// The names of the kinds the used theme has no file for.
    pub missing: Vec<&'static str>,
    /// One per damaged file.
    pub damaged: Vec<Damaged>,
}

/// One log line: a warning for fallbacks, otherwise information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLine {
    pub warn: bool,
    pub text: String,
}

impl ThemeReport {
    /// The log lines, at most one per case (SC-003).
    pub fn lines(&self) -> Vec<ReportLine> {
        let warn = |text: String| ReportLine { warn: true, text };
        let mut lines = Vec::new();
        const OWN: &str = "RubickWay draws its own pointer images";
        match (&self.configured_missing, &self.theme) {
            (Some(c), ThemeUse::NotInstalled) => lines.push(warn(format!(
                "cursor theme \"{c}\" from the configuration is not installed, and there is no \
                 default cursor theme: {OWN}"
            ))),
            (Some(c), ThemeUse::NoImages { .. }) => lines.push(warn(format!(
                "cursor theme \"{c}\" from the configuration is not installed, and the default \
                 cursor theme has no pointer images: {OWN}"
            ))),
            (Some(c), ThemeUse::Used(_)) => lines.push(warn(format!(
                "cursor theme \"{c}\" from the configuration is not installed: using the default \
                 cursor theme"
            ))),
            (None, ThemeUse::NotInstalled) => {
                lines.push(warn(format!("no cursor theme found: {OWN}")));
            }
            (None, ThemeUse::NoImages { name, missing }) => {
                let why = if missing.is_empty() {
                    String::new()
                } else {
                    format!(" (it inherits from {}, not installed)", missing.join(", "))
                };
                lines.push(warn(format!(
                    "cursor theme \"{name}\" has no pointer images{why}: RubickWay draws its own"
                )));
            }
            (None, ThemeUse::Used(_)) => {}
        }
        if let ThemeUse::Used(name) = &self.theme {
            lines.push(ReportLine {
                warn: false,
                text: format!("pointer images from cursor theme \"{name}\""),
            });
            if !self.missing.is_empty() {
                lines.push(warn(format!(
                    "cursor theme \"{name}\" has no images for {}: RubickWay draws its own for \
                     those",
                    self.missing.join(", ")
                )));
            }
            for d in &self.damaged {
                lines.push(warn(format!(
                    "cursor theme \"{name}\": {} is damaged ({}): RubickWay draws its own {} \
                     pointer",
                    d.path.display(),
                    d.reason,
                    d.shape
                )));
            }
        }
        lines
    }
}

/// The name of a kind in log lines.
fn kind_name(kind: CursorKind) -> &'static str {
    match kind {
        CursorKind::Shape(name) => name.css_name(),
        CursorKind::WindowMove => "window move",
    }
}

/// The pointer images: the configured cursor theme (or `default`) where it has an image, the
/// built-in ones otherwise, and what was found for the log. Reads files, so it runs once at
/// startup, before the event loop (Principle VII, cursor-shapes research C6, C12).
pub fn load(icon_dirs: &[PathBuf], configured: Option<&str>) -> (PointerImages, ThemeReport) {
    let mut report = ThemeReport {
        theme: ThemeUse::NotInstalled,
        configured_missing: None,
        missing: Vec::new(),
        damaged: Vec::new(),
    };
    let name = match configured {
        Some(name) if installed(icon_dirs, name) => name,
        Some(name) => {
            report.configured_missing = Some(name.to_string());
            "default"
        }
        None => "default",
    };
    let all_builtin = || PointerImages {
        entries: CursorKind::ALL
            .iter()
            .map(|&kind| (kind, builtin(kind), ImageSource::Builtin))
            .collect(),
    };
    if !installed(icon_dirs, name) {
        return (all_builtin(), report);
    }
    let chain = Chain::resolve(icon_dirs, name);
    let mut entries = Vec::with_capacity(CursorKind::ALL.len());
    let mut found = 0;
    for kind in CursorKind::ALL {
        let Some(path) = chain.find(kind.theme_names()) else {
            report.missing.push(kind_name(kind));
            entries.push((kind, builtin(kind), ImageSource::Builtin));
            continue;
        };
        found += 1;
        match std::fs::read(&path)
            .map_err(|_| XcursorError::CutShort)
            .and_then(|bytes| parse_xcursor(&bytes, CURSOR_SIZE))
        {
            Ok(image) => entries.push((kind, image, ImageSource::Theme)),
            Err(reason) => {
                if !report.damaged.iter().any(|d| d.path == path) {
                    report.damaged.push(Damaged {
                        shape: kind_name(kind),
                        path,
                        reason,
                    });
                }
                entries.push((kind, builtin(kind), ImageSource::Builtin));
            }
        }
    }
    if found == 0 {
        report.theme = ThemeUse::NoImages {
            name: name.to_string(),
            missing: chain.missing,
        };
        report.missing.clear();
        return (all_builtin(), report);
    }
    report.theme = ThemeUse::Used(name.to_string());
    (PointerImages { entries }, report)
}

/// Whether one of the icon directories has a theme of this name.
fn installed(icon_dirs: &[PathBuf], name: &str) -> bool {
    valid_theme_name(name) && icon_dirs.iter().any(|dir| dir.join(name).is_dir())
}

/// A theme and the themes it inherits from: their `cursors/` directories, each listed once.
struct Chain {
    /// In lookup order, with the file names each holds.
    dirs: Vec<(PathBuf, HashSet<OsString>)>,
    /// Inherited themes that are not installed.
    missing: Vec<String>,
}

/// Themes followed through `Inherits=`, at most (as today).
const MAX_THEMES: usize = 8;

impl Chain {
    fn resolve(icon_dirs: &[PathBuf], theme: &str) -> Chain {
        let mut themes = vec![theme.to_string()];
        let mut chain = Chain {
            dirs: Vec::new(),
            missing: Vec::new(),
        };
        let mut i = 0;
        while let Some(name) = themes.get(i).cloned() {
            let mut present = false;
            for dir in icon_dirs {
                let base = dir.join(&name);
                if !base.is_dir() {
                    continue;
                }
                present = true;
                let cursors = base.join("cursors");
                if let Ok(listing) = std::fs::read_dir(&cursors) {
                    let files = listing.flatten().map(|e| e.file_name()).collect();
                    chain.dirs.push((cursors, files));
                }
                if let Ok(index) = std::fs::read_to_string(base.join("index.theme")) {
                    for parent in inherits(&index) {
                        if themes.len() < MAX_THEMES && !themes.contains(&parent) {
                            themes.push(parent);
                        }
                    }
                }
            }
            if !present && i > 0 {
                chain.missing.push(name);
            }
            i += 1;
        }
        chain
    }

    /// The file of the first of `names` the chain has, in order of names, then of themes.
    fn find(&self, names: &[&str]) -> Option<PathBuf> {
        names.iter().find_map(|name| {
            self.dirs
                .iter()
                .find(|(_, files)| files.contains(OsStr::new(name)))
                .map(|(dir, _)| dir.join(name))
        })
    }
}

/// Whether `name` can be a theme directory's name: not empty, `.` or `..`, and without `/` or
/// NUL, so a path built from it stays inside the icon directories (cursor-shapes research C6).
pub fn valid_theme_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\0'])
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

/// The `Inherits=` list of an `index.theme`; names that are not theme names are skipped
/// (research C6).
fn inherits(index: &str) -> Vec<String> {
    index
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Inherits="))
        .flat_map(|v| v.split([',', ';']))
        .map(|s| s.trim().to_string())
        .filter(|s| valid_theme_name(s))
        .collect()
}

/// Why a cursor file cannot be used (research C7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XcursorError {
    NotACursorFile,
    CutShort,
    NoImage,
    TooLarge,
}

impl std::fmt::Display for XcursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            XcursorError::NotACursorFile => "not a cursor file",
            XcursorError::CutShort => "cut short",
            XcursorError::NoImage => "has no image",
            XcursorError::TooLarge => "image larger than 256 pixels",
        })
    }
}

const XCURSOR_IMAGE: u32 = 0xfffd_0002;
/// Images larger than this are not used (no theme ships them; a broken file could claim more).
const XCURSOR_MAX_SIDE: u32 = 256;

/// The image of an Xcursor file whose nominal size is closest to `size` (the first such image of
/// an animated cursor). Every length and offset is checked, so a broken file gives its reason,
/// never a panic.
pub fn parse_xcursor(bytes: &[u8], size: u32) -> Result<CursorImage, XcursorError> {
    use XcursorError::{CutShort, NoImage, NotACursorFile, TooLarge};
    let u32_at = |at: usize| -> Result<u32, XcursorError> {
        let end = at.checked_add(4).ok_or(CutShort)?;
        let b = bytes.get(at..end).ok_or(CutShort)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let add = |a: usize, b: usize| a.checked_add(b).ok_or(CutShort);
    if bytes.get(..4) != Some(b"Xcur") {
        return Err(NotACursorFile);
    }
    let header = u32_at(4)? as usize;
    let ntoc = u32_at(12)? as usize;
    let mut best: Option<(u32, usize)> = None;
    for i in 0..ntoc.min(1024) {
        let entry = add(header, i.checked_mul(12).ok_or(CutShort)?)?;
        if u32_at(entry)? != XCURSOR_IMAGE {
            continue;
        }
        let nominal = u32_at(add(entry, 4)?)?;
        let position = u32_at(add(entry, 8)?)? as usize;
        let better = best.is_none_or(|(n, _)| nominal.abs_diff(size) < n.abs_diff(size));
        if better {
            best = Some((nominal, position));
        }
    }
    let (_, at) = best.ok_or(NoImage)?;
    if u32_at(add(at, 4)?)? != XCURSOR_IMAGE {
        return Err(NotACursorFile);
    }
    let chunk_header = u32_at(at)? as usize;
    let width = u32_at(add(at, 16)?)?;
    let height = u32_at(add(at, 20)?)?;
    let hotspot = (u32_at(add(at, 24)?)?, u32_at(add(at, 28)?)?);
    if width == 0 || height == 0 {
        return Err(NoImage);
    }
    if width > XCURSOR_MAX_SIDE || height > XCURSOR_MAX_SIDE {
        return Err(TooLarge);
    }
    let len = (width as usize) * (height as usize) * 4;
    let start = add(at, chunk_header)?;
    let pixels = bytes.get(start..add(start, len)?).ok_or(CutShort)?.to_vec();
    Ok(CursorImage {
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

    use CursorName as N;

    #[test]
    fn all_kinds_are_listed_once_with_their_own_texture_key() {
        assert_eq!(CursorKind::ALL.len(), 37);
        for (i, kind) in CursorKind::ALL.iter().enumerate() {
            assert_eq!(kind.index(), i, "{kind:?}");
            assert_eq!(kind.texture_key(), u64::MAX - i as u64, "{kind:?}");
        }
        assert_eq!(CursorKind::ALL[8], CursorKind::Shape(N::Text));
        assert_eq!(CursorKind::ALL[36], CursorKind::WindowMove);
    }

    #[test]
    fn rubickways_own_images_look_up_todays_theme_names() {
        // cursor-shapes US3, FR-009: the gestures' theme images do not change.
        let today: [(CursorKind, &[&str]); 7] = [
            (CursorKind::Shape(N::Default), &["default", "left_ptr"]),
            (
                CursorKind::Shape(N::Grabbing),
                &["grabbing", "closedhand", "dnd-move"],
            ),
            (CursorKind::WindowMove, &["move", "fleur", "all-scroll"]),
            (
                CursorKind::Shape(N::EwResize),
                &["ew-resize", "sb_h_double_arrow", "h_double_arrow"],
            ),
            (
                CursorKind::Shape(N::NsResize),
                &["ns-resize", "sb_v_double_arrow", "v_double_arrow"],
            ),
            (
                CursorKind::Shape(N::NwseResize),
                &["nwse-resize", "bd_double_arrow", "size_fdiag"],
            ),
            (
                CursorKind::Shape(N::NeswResize),
                &["nesw-resize", "fd_double_arrow", "size_bdiag"],
            ),
        ];
        for (kind, names) in today {
            assert_eq!(kind.theme_names(), names, "{kind:?}");
        }
    }

    #[test]
    fn todays_images_keep_their_drawings_and_hotspots() {
        let grab = builtin(CursorKind::Shape(N::Grabbing));
        // GRAB[1][2] is '#': the 2x2 block at (4..6, 2..4) is opaque black.
        assert_eq!(pixel(&grab, 4, 2), [0, 0, 0, 255]);
        assert_eq!(grab.hotspot, (12, 12));
        let mv = builtin(CursorKind::WindowMove);
        // MOVE[0][5] is '#', MOVE[0][0] is ' '.
        assert_eq!(pixel(&mv, 10, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&mv, 0, 0), [0, 0, 0, 0]);
        assert_eq!(mv.hotspot, (12, 12));
        let ew = builtin(CursorKind::Shape(N::EwResize));
        // RESIZE_EW[5][0] is '#'.
        assert_eq!(pixel(&ew, 0, 10), [0, 0, 0, 255]);
        assert_eq!(ew.hotspot, (12, 12));
        let ns = builtin(CursorKind::Shape(N::NsResize));
        assert_eq!(pixel(&ns, 10, 0), [0, 0, 0, 255], "transposed");
        let nwse = builtin(CursorKind::Shape(N::NwseResize));
        assert_eq!(pixel(&nwse, 0, 0), [0, 0, 0, 255]);
        let nesw = builtin(CursorKind::Shape(N::NeswResize));
        assert_eq!(pixel(&nesw, 23, 0), [0, 0, 0, 255], "mirrored");
        assert_eq!(builtin(CursorKind::Shape(N::Default)).hotspot, (0, 0));
    }

    #[test]
    fn the_19_drawings_differ_from_each_other() {
        // cursor-shapes FR-006: each recognisable on its own.
        for (i, a) in Art::ALL.iter().enumerate() {
            for b in &Art::ALL[..i] {
                assert_ne!(draw(*a).pixels, draw(*b).pixels, "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn shapes_of_one_meaning_share_a_drawing() {
        // cursor-shapes FR-007.
        let groups: [&[CursorKind]; 9] = [
            &[
                CursorKind::Shape(N::EResize),
                CursorKind::Shape(N::WResize),
                CursorKind::Shape(N::EwResize),
                CursorKind::Shape(N::ColResize),
            ],
            &[
                CursorKind::Shape(N::NResize),
                CursorKind::Shape(N::SResize),
                CursorKind::Shape(N::NsResize),
                CursorKind::Shape(N::RowResize),
            ],
            &[
                CursorKind::Shape(N::NeResize),
                CursorKind::Shape(N::SwResize),
                CursorKind::Shape(N::NeswResize),
            ],
            &[
                CursorKind::Shape(N::NwResize),
                CursorKind::Shape(N::SeResize),
                CursorKind::Shape(N::NwseResize),
            ],
            &[
                CursorKind::Shape(N::NoDrop),
                CursorKind::Shape(N::NotAllowed),
            ],
            &[
                CursorKind::Shape(N::AllScroll),
                CursorKind::Shape(N::AllResize),
                CursorKind::WindowMove,
            ],
            &[
                CursorKind::Shape(N::Default),
                CursorKind::Shape(N::ContextMenu),
                CursorKind::Shape(N::Alias),
                CursorKind::Shape(N::Copy),
                CursorKind::Shape(N::Move),
                CursorKind::Shape(N::DndAsk),
            ],
            &[CursorKind::Shape(N::Grab)],
            &[CursorKind::Shape(N::Grabbing)],
        ];
        for group in groups {
            for kind in group {
                assert_eq!(builtin(*kind), builtin(group[0]), "{kind:?}");
            }
        }
        assert_ne!(
            builtin(CursorKind::Shape(N::Grab)),
            builtin(CursorKind::Shape(N::Grabbing)),
            "open and closed hands"
        );
        // Every drawing is used by some kind.
        for art in Art::ALL {
            assert!(
                CursorKind::ALL.iter().any(|k| art_of(*k) == art),
                "{art:?} unused"
            );
        }
    }

    #[test]
    fn only_the_five_arrow_stand_ins_have_no_drawing_of_their_own() {
        let without: Vec<CursorKind> = CursorKind::ALL
            .into_iter()
            .filter(|k| !has_own_art(*k))
            .collect();
        assert_eq!(
            without,
            vec![
                CursorKind::Shape(N::ContextMenu),
                CursorKind::Shape(N::Alias),
                CursorKind::Shape(N::Copy),
                CursorKind::Shape(N::Move),
                CursorKind::Shape(N::DndAsk),
            ]
        );
    }

    #[test]
    fn hotspots_sit_where_the_shape_points() {
        // cursor-shapes FR-008.
        for kind in CursorKind::ALL {
            let img = builtin(kind);
            match art_of(kind) {
                Art::Arrow | Art::Help | Art::Progress => {
                    assert_eq!(img.hotspot, (0, 0), "{kind:?}: the tip");
                }
                Art::Pointer => {
                    let (x, y) = img.hotspot;
                    assert!(y < 4, "{kind:?}: the fingertip is at the top");
                    assert_eq!(pixel(&img, x, y)[3], 255, "{kind:?}: on the finger");
                }
                Art::ZoomIn | Art::ZoomOut => {
                    let (x, y) = img.hotspot;
                    assert_eq!((x, y), (9, 9), "{kind:?}: the lens centre");
                    // The lens outline is drawn on both sides of it.
                    assert_eq!(pixel(&img, 0, y)[3], 255, "{kind:?}");
                    assert_eq!(pixel(&img, 17, y)[3], 255, "{kind:?}");
                }
                _ => assert_eq!(img.hotspot, (12, 12), "{kind:?}: the centre"),
            }
        }
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
        for art in ALL_ART {
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
        let img = builtin(CursorKind::Shape(N::Default));
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
        assert_ne!(
            builtin(CursorKind::Shape(N::NsResize)),
            builtin(CursorKind::Shape(N::EwResize))
        );
        assert_ne!(
            builtin(CursorKind::Shape(N::NeswResize)),
            builtin(CursorKind::Shape(N::NwseResize))
        );
    }

    #[test]
    fn no_theme_means_builtin_images() {
        let dir = std::env::temp_dir().join("rubickway-no-such-icons");
        let (images, report) = load(&[dir], None);
        assert_eq!(report.theme, ThemeUse::NotInstalled);
        assert_eq!(images.iter().count(), CursorKind::ALL.len());
        for kind in CursorKind::ALL {
            assert_eq!(images.get(kind), Some(&builtin(kind)), "{kind:?}");
            assert_eq!(images.source(kind), Some(ImageSource::Builtin));
        }
        assert!(images.arrow_stand_in(CursorKind::Shape(N::Copy)));
        assert!(!images.arrow_stand_in(CursorKind::Shape(N::Text)));
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
    fn broken_xcursor_files_give_their_reason() {
        use XcursorError::{CutShort, NoImage, NotACursorFile, TooLarge};
        let good = xcursor(24, 24, (0, 0), 0xffff_ffff);
        assert_eq!(parse_xcursor(&good[..good.len() - 1], 24), Err(CutShort));
        assert_eq!(parse_xcursor(b"Xcu", 24), Err(NotACursorFile));
        assert_eq!(
            parse_xcursor(&xcursor(24, 300, (0, 0), 0), 24),
            Err(TooLarge)
        );
        assert_eq!(parse_xcursor(&xcursor(24, 0, (0, 0), 0), 24), Err(NoImage));
        let mut bad = good.clone();
        bad[0] = b'Y';
        assert_eq!(parse_xcursor(&bad, 24), Err(NotACursorFile));
        // No image entry in the table of contents.
        let mut empty = good[..16].to_vec();
        empty[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse_xcursor(&empty, 24), Err(NoImage));
        assert_eq!(TooLarge.to_string(), "image larger than 256 pixels");
    }

    /// Icon directories made for one test, removed when it ends.
    struct Icons(PathBuf);

    impl Icons {
        fn new(tag: &str) -> Icons {
            let root =
                std::env::temp_dir().join(format!("rubickway-icons-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("root");
            Icons(root)
        }

        fn dirs(&self) -> Vec<PathBuf> {
            vec![self.0.clone()]
        }

        fn theme(&self, name: &str, inherits: Option<&str>) {
            std::fs::create_dir_all(self.0.join(name)).expect("theme");
            if let Some(parents) = inherits {
                std::fs::write(
                    self.0.join(name).join("index.theme"),
                    format!("[Icon Theme]\nInherits={parents}\n"),
                )
                .expect("index");
            }
        }

        /// A cursor file with hotspot `(n, n)`, to tell files apart.
        fn cursor(&self, theme: &str, file: &str, n: u32) {
            self.raw(theme, file, &xcursor(24, 24, (n, n), 0xff00_0000));
        }

        fn raw(&self, theme: &str, file: &str, bytes: &[u8]) {
            let dir = self.0.join(theme).join("cursors");
            std::fs::create_dir_all(&dir).expect("cursors");
            std::fs::write(dir.join(file), bytes).expect("cursor");
        }
    }

    impl Drop for Icons {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn hotspot_of(images: &PointerImages, kind: CursorKind) -> (u32, u32) {
        images.get(kind).expect("image").hotspot
    }

    fn texts(report: &ThemeReport) -> Vec<String> {
        report.lines().into_iter().map(|l| l.text).collect()
    }

    #[test]
    fn a_theme_is_found_through_inherits() {
        let icons = Icons::new("inherits");
        icons.theme("default", Some("Adwaita"));
        icons.cursor("Adwaita", "left_ptr", 1);
        let (images, report) = load(&icons.dirs(), None);
        assert_eq!(report.theme, ThemeUse::Used("default".into()));
        assert_eq!(hotspot_of(&images, CursorKind::Shape(N::Default)), (1, 1));
        assert_eq!(
            images.source(CursorKind::Shape(N::Default)),
            Some(ImageSource::Theme)
        );
        assert_eq!(
            images.source(CursorKind::Shape(N::Text)),
            Some(ImageSource::Builtin)
        );
    }

    #[test]
    fn the_chain_is_breadth_first_bounded_and_stops_at_cycles() {
        let icons = Icons::new("chain");
        // a -> b, c; b -> a (cycle); c has the image, and so has d, behind b.
        icons.theme("a", Some("b;c"));
        icons.theme("b", Some("a,d"));
        icons.cursor("c", "text", 3);
        icons.cursor("d", "text", 4);
        let (images, _) = load(&icons.dirs(), Some("a"));
        assert_eq!(
            hotspot_of(&images, CursorKind::Shape(N::Text)),
            (3, 3),
            "c comes before d"
        );
        // Ten themes in a row: only the first eight are followed.
        let icons = Icons::new("long");
        for i in 0..10 {
            icons.theme(&format!("t{i}"), Some(&format!("t{}", i + 1)));
        }
        icons.cursor("t7", "text", 7);
        icons.cursor("t9", "wait", 9);
        let (images, _) = load(&icons.dirs(), Some("t0"));
        assert_eq!(
            images.source(CursorKind::Shape(N::Text)),
            Some(ImageSource::Theme)
        );
        assert_eq!(
            images.source(CursorKind::Shape(N::Wait)),
            Some(ImageSource::Builtin)
        );
    }

    #[test]
    fn inherited_names_that_leave_the_icon_directories_are_skipped() {
        let icons = Icons::new("escape");
        let outside = Icons::new("outside");
        outside.cursor("evil", "text", 5);
        icons.theme(
            "default",
            Some(&format!("{},..,.", outside.0.join("evil").display())),
        );
        icons.cursor("default", "wait", 1);
        let (images, _) = load(&icons.dirs(), None);
        assert_eq!(
            images.source(CursorKind::Shape(N::Text)),
            Some(ImageSource::Builtin)
        );
    }

    #[test]
    fn names_are_looked_up_in_order_with_the_axis_last() {
        let icons = Icons::new("names");
        icons.theme("default", None);
        icons.cursor("default", "ew-resize", 1);
        icons.cursor("default", "w-resize", 2);
        icons.cursor("default", "not-allowed", 3);
        let (images, _) = load(&icons.dirs(), None);
        assert_eq!(hotspot_of(&images, CursorKind::Shape(N::WResize)), (2, 2));
        assert_eq!(
            hotspot_of(&images, CursorKind::Shape(N::EResize)),
            (1, 1),
            "the axis"
        );
        assert_eq!(hotspot_of(&images, CursorKind::Shape(N::ColResize)), (1, 1));
        assert_eq!(hotspot_of(&images, CursorKind::Shape(N::NoDrop)), (3, 3));
    }

    #[test]
    fn a_configured_theme_that_is_not_installed_falls_back_to_default() {
        let icons = Icons::new("typo");
        icons.theme("default", None);
        icons.cursor("default", "left_ptr", 1);
        let (images, report) = load(&icons.dirs(), Some("Adwaitaa"));
        assert_eq!(report.configured_missing.as_deref(), Some("Adwaitaa"));
        assert_eq!(report.theme, ThemeUse::Used("default".into()));
        assert_eq!(hotspot_of(&images, CursorKind::Shape(N::Default)), (1, 1));
        assert_eq!(
            texts(&report)[..2],
            [
                "cursor theme \"Adwaitaa\" from the configuration is not installed: using the \
                 default cursor theme",
                "pointer images from cursor theme \"default\"",
            ]
        );
        // Without a default theme either: one line, built-in images.
        let empty = Icons::new("typo-empty");
        let (images, report) = load(&empty.dirs(), Some("Adwaitaa"));
        assert_eq!(
            images.source(CursorKind::Shape(N::Default)),
            Some(ImageSource::Builtin)
        );
        assert_eq!(
            texts(&report),
            [
                "cursor theme \"Adwaitaa\" from the configuration is not installed, and there is \
                 no default cursor theme: RubickWay draws its own pointer images"
            ]
        );
    }

    #[test]
    fn a_theme_with_no_images_counts_as_none_not_as_default() {
        // The laptop: `default` inherits Adwaita, which is not installed.
        let icons = Icons::new("laptop");
        icons.theme("default", Some("Adwaita"));
        let (images, report) = load(&icons.dirs(), None);
        assert_eq!(
            report.theme,
            ThemeUse::NoImages {
                name: "default".into(),
                missing: vec!["Adwaita".into()]
            }
        );
        assert!(report.missing.is_empty());
        for kind in CursorKind::ALL {
            assert_eq!(images.source(kind), Some(ImageSource::Builtin), "{kind:?}");
        }
        assert_eq!(
            texts(&report),
            [
                "cursor theme \"default\" has no pointer images (it inherits from Adwaita, not \
                 installed): RubickWay draws its own"
            ],
            "exactly one line (SC-003)"
        );
        // A configured theme without images: the built-in images, even with a default theme.
        icons.theme("empty", None);
        icons.cursor("default", "left_ptr", 1);
        let (images, report) = load(&icons.dirs(), Some("empty"));
        assert_eq!(
            report.theme,
            ThemeUse::NoImages {
                name: "empty".into(),
                missing: vec![]
            }
        );
        assert_eq!(
            images.source(CursorKind::Shape(N::Default)),
            Some(ImageSource::Builtin)
        );
        assert_eq!(
            texts(&report),
            ["cursor theme \"empty\" has no pointer images: RubickWay draws its own"]
        );
    }

    #[test]
    fn no_theme_at_all_gives_exactly_one_line() {
        let empty = Icons::new("none");
        let (_, report) = load(&empty.dirs(), None);
        let lines = report.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].warn);
        assert_eq!(
            lines[0].text,
            "no cursor theme found: RubickWay draws its own pointer images"
        );
    }

    #[test]
    fn missing_shapes_are_named_in_one_line() {
        let icons = Icons::new("missing");
        icons.theme("default", None);
        // Every kind but text, vertical-text and dnd-ask has a file.
        for kind in CursorKind::ALL {
            if !matches!(
                kind,
                CursorKind::Shape(N::Text | N::VerticalText | N::DndAsk)
            ) {
                icons.cursor("default", kind.theme_names()[0], 1);
            }
        }
        let (images, report) = load(&icons.dirs(), None);
        assert_eq!(report.missing, ["text", "vertical-text", "dnd-ask"]);
        assert_eq!(
            images.source(CursorKind::Shape(N::Text)),
            Some(ImageSource::Builtin)
        );
        let lines = texts(&report);
        assert_eq!(
            lines,
            [
                "pointer images from cursor theme \"default\"",
                "cursor theme \"default\" has no images for text, vertical-text, dnd-ask: \
                 RubickWay draws its own for those",
            ]
        );
        // The theme has an image for the arrow stand-ins too: no first-use line for them.
        assert!(!images.arrow_stand_in(CursorKind::Shape(N::Copy)));
    }

    #[test]
    fn a_damaged_file_gives_the_builtin_image_and_one_line_per_file() {
        let icons = Icons::new("damaged");
        icons.theme("default", None);
        icons.raw("default", "text", b"Xcur");
        icons.cursor("default", "xterm", 1);
        // fleur serves all-scroll, all-resize and the window move.
        icons.raw("default", "fleur", b"not a cursor");
        let (images, report) = load(&icons.dirs(), None);
        assert_eq!(
            images.get(CursorKind::Shape(N::Text)),
            Some(&builtin(CursorKind::Shape(N::Text))),
            "the first file found decides"
        );
        assert_eq!(
            report.damaged,
            [
                Damaged {
                    shape: "text",
                    path: icons.0.join("default/cursors/text"),
                    reason: XcursorError::CutShort,
                },
                Damaged {
                    shape: "all-scroll",
                    path: icons.0.join("default/cursors/fleur"),
                    reason: XcursorError::NotACursorFile,
                },
            ]
        );
        let lines = texts(&report);
        assert!(
            lines.contains(&format!(
                "cursor theme \"default\": {} is damaged (cut short): RubickWay draws its own \
                 text pointer",
                icons.0.join("default/cursors/text").display()
            )),
            "{lines:?}"
        );
    }
}
