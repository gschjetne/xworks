//! Raster images, ready to draw: sRGB, premultiplied, at most
//! [`MAX_SIDE`] pixels a side, with smaller copies for drawing them small.

use std::sync::Arc;

/// The longest side an [`Image`] keeps, in pixels. Larger pictures are
/// scaled down to it when they're made: render-gpu keeps each image in a
/// vello_gpu atlas page of 4 096 pixels a side, with a pixel of border
/// around it, and at 4 094 pixels across a picture still prints sharp at
/// 300 dots to the inch over 34 centimetres.
pub const MAX_SIDE: u32 = 4094;

/// Why an image couldn't be made.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    /// A side of no pixels.
    #[error("an image of {width} by {height} pixels has no pixels")]
    Empty {
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
    /// Pixels that don't fill the size given.
    #[error("{width} by {height} pixels take four bytes each, not {got} bytes in all")]
    Size {
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
        /// How many bytes came.
        got: usize,
    },
}

/// A raster image: sRGB, 8 bits a channel, premultiplied by its alpha,
/// at most [`MAX_SIDE`] pixels a side. It holds copies of itself, each
/// half the size of the one before, down to a pixel: renderers draw from
/// the smallest that's still at least as large as what they draw, so
/// pictures drawn small don't shimmer, as mipmaps would do on a GPU.
///
/// Cloning shares the pixels.
#[derive(Clone)]
pub struct Image(Arc<Raster>);

/// The bytes of a file an image was decoded from, shared.
pub type FileData = Arc<dyn AsRef<[u8]> + Send + Sync>;

struct Raster {
    levels: Vec<Level>,
    opaque: bool,
    /// The sRGB JPEG file the pixels were decoded from, if it's kept.
    jpeg: Option<FileData>,
}

/// One size of an [`Image`]: premultiplied RGBA, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    /// Its width, in pixels.
    pub width: u32,
    /// Its height, in pixels.
    pub height: u32,
    /// Its pixels, four bytes each, premultiplied.
    pub pixels: Vec<u8>,
}

impl Image {
    /// An image `width` by `height` pixels of straight-alpha sRGB `rgba`,
    /// row by row: scaled down to fit [`MAX_SIDE`] if it's larger, keeping
    /// its proportions, and with its smaller copies made.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Image, ImageError> {
        Image::make(width, height, rgba, None)
    }

    /// An image as [`Image::new`] makes it of the pixels decoded from
    /// `jpeg`, an sRGB JPEG file shown as it's stored, which it keeps:
    /// writers that can hold the file as it is, as PDF can, write it
    /// rather than the pixels, keeping its size (project owner,
    /// 2026-10-23, review dcn5ejrd). Screens draw the pixels.
    pub fn with_jpeg(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        jpeg: FileData,
    ) -> Result<Image, ImageError> {
        Image::make(width, height, rgba, Some(jpeg))
    }

    fn make(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        jpeg: Option<FileData>,
    ) -> Result<Image, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty { width, height });
        }
        let want = u64::from(width) * u64::from(height) * 4;
        if rgba.len() as u64 != want {
            return Err(ImageError::Size {
                width,
                height,
                got: rgba.len(),
            });
        }
        let mut pixels = rgba;
        premultiply(&mut pixels);
        let opaque = pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
        let (w, h) = fit(width, height);
        let mut level = if (w, h) == (width, height) {
            Level {
                width,
                height,
                pixels,
            }
        } else {
            Level {
                width: w,
                height: h,
                pixels: resample(&pixels, width, height, w, h),
            }
        };
        let mut levels = Vec::new();
        while level.width > 1 || level.height > 1 {
            let (w, h) = (level.width.div_ceil(2), level.height.div_ceil(2));
            let next = Level {
                width: w,
                height: h,
                pixels: resample(&level.pixels, level.width, level.height, w, h),
            };
            levels.push(level);
            level = next;
        }
        levels.push(level);
        Ok(Image(Arc::new(Raster {
            levels,
            opaque,
            jpeg,
        })))
    }

    /// The sRGB JPEG file the image was decoded from, if it keeps one
    /// ([`Image::with_jpeg`]).
    pub fn jpeg(&self) -> Option<&[u8]> {
        self.0.jpeg.as_deref().map(|d| d.as_ref())
    }

    /// The same, shared, for writers that keep it.
    pub fn jpeg_data(&self) -> Option<FileData> {
        self.0.jpeg.clone()
    }

    /// Its width in pixels, at its largest.
    pub fn width(&self) -> u32 {
        self.0.levels[0].width
    }

    /// Its height in pixels, at its largest.
    pub fn height(&self) -> u32 {
        self.0.levels[0].height
    }

    /// Its sizes, largest first, each half the one before, rounded up,
    /// down to a pixel.
    pub fn levels(&self) -> &[Level] {
        &self.0.levels
    }

    /// Whether every pixel is opaque.
    pub fn opaque(&self) -> bool {
        self.0.opaque
    }

    /// Which of its levels to draw it from when it's drawn `width` by
    /// `height` pixels: the smallest at least that large, or the largest
    /// if it's drawn larger still.
    pub fn level_for(&self, width: f64, height: f64) -> usize {
        self.0
            .levels
            .iter()
            .rposition(|l| f64::from(l.width) >= width && f64::from(l.height) >= height)
            .unwrap_or(0)
    }

    /// Its largest level with straight alpha, as RGB and, unless it's
    /// opaque, alpha, for writers that keep the two apart.
    pub fn straight(&self) -> (Vec<u8>, Option<Vec<u8>>) {
        let pixels = &self.0.levels[0].pixels;
        let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(if self.opaque() { 0 } else { pixels.len() / 4 });
        for p in pixels.as_chunks::<4>().0 {
            let a = u32::from(p[3]);
            let un = |c: u8| match a {
                0 => 0,
                255 => c,
                _ => ((u32::from(c) * 255 + a / 2) / a).min(255) as u8,
            };
            rgb.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2])]);
            if !self.opaque() {
                alpha.push(p[3]);
            }
        }
        (rgb, (!self.opaque()).then_some(alpha))
    }

    /// Whether `self` and `other` are the same image, by identity, not by
    /// comparing pixels.
    pub fn same(&self, other: &Image) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// A key identifying the image while it lives, for renderers to cache
    /// what they make of it; they keep a clone while they do, so the key
    /// can't come to mean another image.
    pub fn key(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("levels", &self.0.levels.len())
            .field("opaque", &self.0.opaque)
            .field("jpeg", &self.jpeg().map(<[u8]>::len))
            .finish()
    }
}

/// The size `width` by `height` scaled to fit [`MAX_SIDE`], keeping its
/// proportions, rounded to the nearest pixel and at least one.
fn fit(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= MAX_SIDE {
        return (width, height);
    }
    let scale = |side: u32| {
        let n = u64::from(side) * u64::from(MAX_SIDE);
        let d = u64::from(longest);
        ((n + d / 2) / d).max(1) as u32
    };
    (scale(width), scale(height))
}

/// Multiplies each pixel's colour by its alpha, rounding to the nearest.
fn premultiply(pixels: &mut [u8]) {
    for p in pixels.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a == 255 {
            continue;
        }
        for c in &mut p[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
}

/// `src`, `sw` by `sh` premultiplied pixels, scaled to `dw` by `dh`, no
/// larger, by averaging the area each new pixel covers. The arithmetic is
/// exact, in integers, so every platform gets the same pixels.
pub(crate) fn resample(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    debug_assert!(dw <= sw && dh <= sh && dw > 0 && dh > 0);
    let (sw_, sh_, dw_, dh_) = (sw as usize, sh as usize, dw as usize, dh as usize);
    // Across: each new column sums the old ones it covers, weighted by how
    // much of each, in units where an old pixel is `dw` wide and a new one
    // `sw`. The weights of a column add up to `sw`.
    let across = spans(sw, dw);
    let mut rows = vec![0u32; dw_ * sh_ * 4];
    for y in 0..sh_ {
        let line = &src[y * sw_ * 4..(y + 1) * sw_ * 4];
        let out = &mut rows[y * dw_ * 4..(y + 1) * dw_ * 4];
        for (x, span) in across.iter().enumerate() {
            let mut sum = [0u32; 4];
            for &(i, w) in span {
                let p = &line[i * 4..i * 4 + 4];
                for c in 0..4 {
                    sum[c] += u32::from(p[c]) * w;
                }
            }
            out[x * 4..x * 4 + 4].copy_from_slice(&sum);
        }
    }
    // Down, likewise; the weights add up to `sh`, so a pixel's sum is
    // divided by `sw` times `sh`.
    let down = spans(sh, dh);
    let total = u64::from(sw) * u64::from(sh);
    let mut out = vec![0u8; dw_ * dh_ * 4];
    for (y, span) in down.iter().enumerate() {
        for x in 0..dw_ {
            let mut sum = [0u64; 4];
            for &(j, w) in span {
                let p = &rows[(j * dw_ + x) * 4..(j * dw_ + x) * 4 + 4];
                for c in 0..4 {
                    sum[c] += u64::from(p[c]) * u64::from(w);
                }
            }
            for c in 0..4 {
                out[(y * dw_ + x) * 4 + c] = ((sum[c] + total / 2) / total) as u8;
            }
        }
    }
    out
}

/// For each of `to` new pixels along a side of `from` old ones, the old
/// pixels it covers and by how much: old pixel `i` spans `i * to` to
/// `(i + 1) * to`, and new pixel `x` spans `x * from` to `(x + 1) * from`.
fn spans(from: u32, to: u32) -> Vec<Vec<(usize, u32)>> {
    let (from64, to64) = (u64::from(from), u64::from(to));
    (0..to64)
        .map(|x| {
            let (start, end) = (x * from64, (x + 1) * from64);
            let first = start / to64;
            let last = (end - 1) / to64;
            (first..=last)
                .map(|i| {
                    let (a, b) = (i * to64, (i + 1) * to64);
                    let w = end.min(b) - start.max(a);
                    (i as usize, w as u32)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        rgba.repeat((width * height) as usize)
    }

    #[test]
    fn an_image_needs_pixels_and_the_right_number_of_them() {
        assert_eq!(
            Image::new(0, 4, Vec::new()).err(),
            Some(ImageError::Empty {
                width: 0,
                height: 4
            })
        );
        assert_eq!(
            Image::new(2, 2, vec![0; 15]).err(),
            Some(ImageError::Size {
                width: 2,
                height: 2,
                got: 15
            })
        );
    }

    #[test]
    fn levels_halve_down_to_a_pixel() {
        let image = Image::new(5, 3, solid(5, 3, [10, 20, 30, 255])).unwrap();
        let sizes: Vec<(u32, u32)> = image.levels().iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, [(5, 3), (3, 2), (2, 1), (1, 1)]);
        // A flat colour stays that colour at every size.
        for level in image.levels() {
            assert!(
                level
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p == &[10, 20, 30, 255])
            );
        }
        assert!(image.opaque());
    }

    #[test]
    fn a_large_image_is_scaled_to_fit_keeping_its_proportions() {
        let image = Image::new(MAX_SIDE * 2, 3, solid(MAX_SIDE * 2, 3, [0, 0, 0, 255])).unwrap();
        assert_eq!((image.width(), image.height()), (MAX_SIDE, 2));
        let image = Image::new(1, MAX_SIDE + 1, solid(1, MAX_SIDE + 1, [0, 0, 0, 255])).unwrap();
        assert_eq!((image.width(), image.height()), (1, MAX_SIDE));
        // At the limit, it's kept as it is.
        let image = Image::new(MAX_SIDE, 1, solid(MAX_SIDE, 1, [0, 0, 0, 255])).unwrap();
        assert_eq!((image.width(), image.height()), (MAX_SIDE, 1));
    }

    #[test]
    fn averaging_weighs_the_area_each_pixel_covers() {
        // Black, white, black: halved to two pixels, each covers one and a
        // half old ones, half of them white.
        let src = [[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 255]].concat();
        let out = resample(&src, 3, 1, 2, 1);
        assert_eq!(out, [[85, 85, 85, 255], [85, 85, 85, 255]].concat());
        // Four into one.
        let src = [
            [0, 0, 0, 0],
            [255, 0, 0, 255],
            [0, 0, 0, 0],
            [0, 0, 255, 255],
        ]
        .concat();
        assert_eq!(resample(&src, 2, 2, 1, 1), [64, 0, 64, 128]);
    }

    #[test]
    fn colour_is_premultiplied_and_comes_back_straight() {
        let image = Image::new(1, 1, vec![200, 100, 50, 128]).unwrap();
        assert_eq!(image.levels()[0].pixels, [100, 50, 25, 128]);
        assert!(!image.opaque());
        let (rgb, alpha) = image.straight();
        assert_eq!(alpha, Some(vec![128]));
        for (got, want) in rgb.iter().zip([200u8, 100, 50]) {
            assert!(got.abs_diff(want) <= 1, "{got} for {want}");
        }
        let opaque = Image::new(1, 1, vec![1, 2, 3, 255]).unwrap();
        assert_eq!(opaque.straight(), (vec![1, 2, 3], None));
    }

    #[test]
    fn a_jpeg_is_kept_only_where_its_given() {
        let plain = Image::new(1, 1, vec![1, 2, 3, 255]).unwrap();
        assert!(plain.jpeg().is_none());
        let file: FileData = Arc::new(b"\xff\xd8\xff the file".to_vec());
        let kept = Image::with_jpeg(1, 1, vec![1, 2, 3, 255], file).unwrap();
        assert_eq!(kept.jpeg(), Some(b"\xff\xd8\xff the file".as_slice()));
        assert_eq!(kept.levels(), plain.levels(), "the same pixels drawn");
        assert!(kept.clone().jpeg_data().is_some(), "clones share it");
    }

    #[test]
    fn the_level_drawn_is_the_smallest_still_large_enough() {
        let image = Image::new(64, 32, solid(64, 32, [0; 4])).unwrap();
        assert_eq!(image.level_for(64.0, 32.0), 0);
        assert_eq!(image.level_for(200.0, 100.0), 0);
        assert_eq!(image.level_for(33.0, 16.0), 0);
        assert_eq!(image.level_for(32.0, 16.0), 1);
        assert_eq!(image.level_for(31.5, 3.0), 1);
        assert_eq!(image.level_for(8.0, 4.0), 3);
        assert_eq!(image.level_for(0.5, 0.5), 6);
    }
}
