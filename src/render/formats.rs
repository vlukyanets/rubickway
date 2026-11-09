//! What the GPU can read, as applications are told (research G2). Pure.

/// A format and memory layout: `DRM_FORMAT_*` and `DRM_FORMAT_MOD_*`.
pub type FormatPair = (u32, u64);

/// The layout chosen by the driver, unknown to anyone else.
pub const MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

/// Bytes of one entry of the format table: format, padding, modifier.
const ENTRY_SIZE: usize = 16;

/// Indices in `tranche_formats` are 16 bits.
const MAX_ENTRIES: usize = 1 << 16;

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*code)
}

/// The pairs to offer, given each format the GPU samples and its modifiers (external-only ones
/// included). A format reporting no modifiers gets one `DRM_FORMAT_MOD_INVALID` entry; a format
/// reporting some is offered with exactly those, because implicit layouts do not work across GPUs.
pub fn table(formats: &[(u32, Vec<u64>)]) -> Vec<FormatPair> {
    let mut pairs: Vec<FormatPair> = Vec::new();
    for (format, modifiers) in formats {
        if modifiers.is_empty() {
            pairs.push((*format, MOD_INVALID));
        } else {
            pairs.extend(modifiers.iter().map(|m| (*format, *m)));
        }
    }
    pairs.truncate(MAX_ENTRIES);
    pairs
}

/// The format table as `zwp_linux_dmabuf_feedback_v1.format_table` lays it out.
pub fn table_bytes(pairs: &[FormatPair]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pairs.len() * ENTRY_SIZE);
    for (format, modifier) in pairs {
        bytes.extend_from_slice(&format.to_ne_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&modifier.to_ne_bytes());
    }
    bytes
}

/// The log's words for an image the GPU would not read. The driver's error is left out so that one
/// application's failures with one layout stay one cause (FR-015).
pub fn refusal(gpu: &str, format: u32, modifier: u64) -> String {
    let name: String = format
        .to_le_bytes()
        .iter()
        .map(|b| match b {
            0x21..=0x7e => char::from(*b),
            _ => '?',
        })
        .collect();
    format!("{gpu} cannot read its image ({name} {modifier:#x})")
}

/// How many planes `format` needs at least. More may come with the modifier (compression
/// planes); the driver checks those when it imports (research G3).
pub fn min_planes(format: u32) -> usize {
    const TWO: [u32; 8] = [
        fourcc(b"NV12"),
        fourcc(b"NV21"),
        fourcc(b"NV16"),
        fourcc(b"NV61"),
        fourcc(b"NV24"),
        fourcc(b"NV42"),
        fourcc(b"P010"),
        fourcc(b"P012"),
    ];
    const THREE: [u32; 8] = [
        fourcc(b"YU12"),
        fourcc(b"YV12"),
        fourcc(b"YU16"),
        fourcc(b"YV16"),
        fourcc(b"YU24"),
        fourcc(b"YV24"),
        fourcc(b"P016"),
        fourcc(b"Q410"),
    ];
    if TWO.contains(&format) {
        2
    } else if THREE.contains(&format) {
        3
    } else {
        1
    }
}

/// What to offer given what the renderer reports: nothing if it cannot import, or has no format
/// to import (FR-014).
pub fn offer(can_import: bool, pairs: Vec<FormatPair>) -> Option<Vec<FormatPair>> {
    (can_import && !pairs.is_empty()).then_some(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARGB: u32 = fourcc(b"AR24");
    const XRGB: u32 = fourcc(b"XR24");
    const LINEAR: u64 = 0;
    const TILED: u64 = 0x0100_0000_0000_0001;

    #[test]
    fn a_refusal_names_the_gpu_format_and_layout() {
        assert_eq!(
            refusal("Intel", ARGB, TILED),
            "Intel cannot read its image (AR24 0x100000000000001)"
        );
        // An application's format number is any number at all.
        assert_eq!(
            refusal("Intel", 0x0000_ff41, 0),
            "Intel cannot read its image (A??? 0x0)"
        );
    }

    #[test]
    fn modifiers_become_pairs_and_no_modifiers_become_invalid() {
        let pairs = table(&[(ARGB, vec![LINEAR, TILED]), (XRGB, vec![])]);
        assert_eq!(
            pairs,
            vec![(ARGB, LINEAR), (ARGB, TILED), (XRGB, MOD_INVALID)]
        );
    }

    #[test]
    fn indices_stay_within_16_bits() {
        let many = vec![(ARGB, (0..70_000u64).collect::<Vec<_>>())];
        assert_eq!(table(&many).len(), 1 << 16);
    }

    #[test]
    fn the_table_lays_out_format_padding_modifier() {
        let bytes = table_bytes(&[(ARGB, TILED)]);
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[..4], ARGB.to_ne_bytes());
        assert_eq!(bytes[4..8], [0; 4]);
        assert_eq!(bytes[8..16], TILED.to_ne_bytes());
    }

    #[test]
    fn video_formats_need_their_planes() {
        assert_eq!(min_planes(ARGB), 1);
        assert_eq!(min_planes(fourcc(b"NV12")), 2);
        assert_eq!(min_planes(fourcc(b"YU12")), 3);
    }

    #[test]
    fn a_renderer_that_cannot_import_gets_no_offer() {
        assert_eq!(offer(false, vec![(ARGB, LINEAR)]), None);
        assert_eq!(offer(true, vec![]), None);
        assert_eq!(
            offer(true, vec![(ARGB, LINEAR)]),
            Some(vec![(ARGB, LINEAR)])
        );
    }
}
