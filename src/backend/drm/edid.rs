//! Make and model from a display's EDID (research S13). Pure.

/// What RubickWay shows of a display's identity (FR-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdidInfo {
    /// The three-letter PNP manufacturer id, e.g. `CMN`.
    pub make: String,
    /// The monitor name descriptor, e.g. `DELL U2720Q`.
    pub model: String,
}

/// Used for a part the display does not report.
pub(crate) const UNKNOWN: &str = "Unknown";

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
const BLOCK: usize = 128;
/// The four 18-byte descriptors of the base block.
const DESCRIPTORS: [usize; 4] = [54, 72, 90, 108];
const MONITOR_NAME: u8 = 0xFC;

/// Reads make and model from an EDID blob; anything missing or malformed is `Unknown`.
pub(crate) fn parse(bytes: &[u8]) -> EdidInfo {
    let unknown = || EdidInfo {
        make: UNKNOWN.into(),
        model: UNKNOWN.into(),
    };
    let Some(base) = bytes.get(..BLOCK) else {
        return unknown();
    };
    if base.get(..8) != Some(&HEADER[..]) {
        return unknown();
    }
    EdidInfo {
        make: manufacturer(base).unwrap_or_else(|| UNKNOWN.into()),
        model: monitor_name(base).unwrap_or_else(|| UNKNOWN.into()),
    }
}

/// Bytes 8-9, big-endian: three letters of five bits each, `1` = `A`.
fn manufacturer(base: &[u8]) -> Option<String> {
    let id = u16::from_be_bytes([*base.get(8)?, *base.get(9)?]);
    let letter = |shift: u16| {
        let v = ((id >> shift) & 0x1F) as u8;
        (1..=26).contains(&v).then(|| char::from(b'A' + v - 1))
    };
    Some([letter(10)?, letter(5)?, letter(0)?].iter().collect())
}

/// The monitor name descriptor: `00 00 00 FC 00` then up to 13 bytes, ended by `0x0A`.
fn monitor_name(base: &[u8]) -> Option<String> {
    DESCRIPTORS.iter().find_map(|&at| {
        let d = base.get(at..at + 18)?;
        if d.get(..3) != Some(&[0, 0, 0][..]) || d.get(3) != Some(&MONITOR_NAME) {
            return None;
        }
        let text = d.get(5..)?;
        let end = text.iter().position(|b| *b == 0x0A).unwrap_or(text.len());
        let name: String = text
            .get(..end)?
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() || *b == b' ' {
                    char::from(*b)
                } else {
                    '?'
                }
            })
            .collect();
        let name = name.trim().to_string();
        (!name.is_empty()).then_some(name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A base block with manufacturer `DEL` and the given name descriptor in slot 2.
    fn block(name: Option<&[u8]>) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&HEADER);
        // D=4, E=5, L=12: 00100 00101 01100 -> 0b0_00100_00101_01100 = 0x10AC.
        b[8] = 0x10;
        b[9] = 0xAC;
        if let Some(name) = name {
            let at = 72;
            b[at..at + 5].copy_from_slice(&[0, 0, 0, 0xFC, 0]);
            b[at + 5..at + 5 + name.len()].copy_from_slice(name);
        }
        b
    }

    #[test]
    fn make_and_model_from_a_valid_block() {
        let info = parse(&block(Some(b"DELL U2720Q\n  ")));
        assert_eq!(info.make, "DEL");
        assert_eq!(info.model, "DELL U2720Q");
    }

    #[test]
    fn a_name_filling_all_13_bytes_has_no_terminator() {
        let info = parse(&block(Some(b"ABCDEFGHIJKLM")));
        assert_eq!(info.model, "ABCDEFGHIJKLM");
    }

    #[test]
    fn missing_parts_are_unknown() {
        assert_eq!(parse(&block(None)).model, UNKNOWN);
        assert_eq!(parse(&block(None)).make, "DEL");
        let mut bad = block(Some(b"X\n"));
        bad[0] = 0x01;
        assert_eq!(
            parse(&bad),
            EdidInfo {
                make: UNKNOWN.into(),
                model: UNKNOWN.into()
            }
        );
        assert_eq!(parse(&block(Some(b"X\n"))[..100]).make, UNKNOWN);
        assert_eq!(parse(&[]).model, UNKNOWN);
    }

    #[test]
    fn an_invalid_manufacturer_letter_is_unknown() {
        let mut b = block(None);
        b[8] = 0x00;
        b[9] = 0x00;
        assert_eq!(parse(&b).make, UNKNOWN);
    }

    proptest! {
        #[test]
        fn never_panics(bytes in prop::collection::vec(any::<u8>(), 0..300)) {
            let _ = parse(&bytes);
            let mut with_header = HEADER.to_vec();
            with_header.extend(&bytes);
            let _ = parse(&with_header);
        }
    }
}
