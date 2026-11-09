//! A GPU image as the renderer sees it: a plain description, knowing nothing of Wayland, and the
//! attributes EGL imports it with (research G4).

use std::os::fd::{AsRawFd, BorrowedFd};

use super::formats::MOD_INVALID;

const WIDTH: i32 = 0x3057;
const HEIGHT: i32 = 0x3056;
const NONE: i32 = 0x3038;
const LINUX_DRM_FOURCC: i32 = 0x3271;

/// `EGL_DMA_BUF_PLANEn_{FD,OFFSET,PITCH,MODIFIER_LO,MODIFIER_HI}_EXT`, plane by plane.
const PLANES: [[i32; 5]; 4] = [
    [0x3272, 0x3273, 0x3274, 0x3443, 0x3444],
    [0x3275, 0x3276, 0x3277, 0x3445, 0x3446],
    [0x3278, 0x3279, 0x327A, 0x3447, 0x3448],
    [0x3440, 0x3441, 0x3442, 0x3449, 0x344A],
];

/// One plane of an image.
#[derive(Debug, Clone, Copy)]
pub struct PlaneDesc<'a> {
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

/// An image to import.
#[derive(Debug, Clone, Copy)]
pub struct ImageDesc<'a> {
    pub width: i32,
    pub height: i32,
    pub fourcc: u32,
    pub modifier: u64,
    pub planes: &'a [PlaneDesc<'a>],
}

/// The attribute list for `eglCreateImageKHR`: `None` if a value does not fit, or there are more
/// planes than the extension has.
pub(crate) fn dma_buf_attribs(desc: &ImageDesc<'_>) -> Option<Vec<i32>> {
    if desc.planes.is_empty() || desc.planes.len() > PLANES.len() {
        return None;
    }
    let mut attribs = vec![
        WIDTH,
        desc.width,
        HEIGHT,
        desc.height,
        LINUX_DRM_FOURCC,
        desc.fourcc as i32,
    ];
    for (plane, names) in desc.planes.iter().zip(&PLANES) {
        attribs.extend([names[0], plane.fd.as_raw_fd()]);
        attribs.extend([names[1], i32::try_from(plane.offset).ok()?]);
        attribs.extend([names[2], i32::try_from(plane.stride).ok()?]);
        if desc.modifier != MOD_INVALID {
            attribs.extend([names[3], desc.modifier as u32 as i32]);
            attribs.extend([names[4], (desc.modifier >> 32) as u32 as i32]);
        }
    }
    attribs.push(NONE);
    Some(attribs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    fn desc<'a>(planes: &'a [PlaneDesc<'a>], modifier: u64) -> ImageDesc<'a> {
        ImageDesc {
            width: 64,
            height: 8,
            fourcc: 0x3432_5241,
            modifier,
            planes,
        }
    }

    #[test]
    fn a_plane_with_a_modifier_names_all_of_it() {
        let file = std::fs::File::open("/proc/self/exe").unwrap();
        let planes = [PlaneDesc {
            fd: file.as_fd(),
            offset: 16,
            stride: 256,
        }];
        let attribs = dma_buf_attribs(&desc(&planes, 0x0100_0000_0000_0002)).unwrap();
        assert_eq!(
            attribs,
            vec![
                WIDTH,
                64,
                HEIGHT,
                8,
                LINUX_DRM_FOURCC,
                0x3432_5241,
                0x3272,
                file.as_raw_fd(),
                0x3273,
                16,
                0x3274,
                256,
                0x3443,
                2,
                0x3444,
                0x0100_0000,
                NONE,
            ]
        );
    }

    #[test]
    fn the_driver_chooses_when_the_modifier_is_invalid() {
        let file = std::fs::File::open("/proc/self/exe").unwrap();
        let planes = [PlaneDesc {
            fd: file.as_fd(),
            offset: 0,
            stride: 4,
        }];
        let attribs = dma_buf_attribs(&desc(&planes, MOD_INVALID)).unwrap();
        assert!(!attribs.contains(&0x3443));
    }

    #[test]
    fn values_that_do_not_fit_are_refused() {
        let file = std::fs::File::open("/proc/self/exe").unwrap();
        let planes = [PlaneDesc {
            fd: file.as_fd(),
            offset: u32::MAX,
            stride: 4,
        }];
        assert!(dma_buf_attribs(&desc(&planes, MOD_INVALID)).is_none());
        assert!(dma_buf_attribs(&desc(&[], MOD_INVALID)).is_none());
    }
}
