//! Pictures read from their files' bytes: PNG, JPEG, GIF and WebP, a GIF's
//! first frame, decoded and converted to sRGB, ready to place
//! (docs/ARCHITECTURE.md, section 10; project owner, 2026-10-22, review
//! pysc65hp).
//!
//! Invariants:
//!
//! - A picture larger than [`Limits::most_pixels`] is refused from its
//!   header, before its pixels are decoded.
//! - Malformed bytes give an error, never a panic.
//! - Colour is converted from the picture's ICC profile, where it has an
//!   RGB or grey one, to sRGB; one that can't be read is taken as sRGB, as
//!   is a picture without one.
//! - A JPEG is turned as its EXIF orientation says, as cameras expect.
//! - An sRGB JPEG shown as it's stored, a colour one without a profile or
//!   with one that converts to sRGB as itself, keeps its file
//!   ([`scene::Image::jpeg`]), for PDF to hold unchanged (project owner,
//!   2026-10-23, review dcn5ejrd).

use std::io::Cursor;
use std::num::NonZeroU64;
use std::sync::Arc;

use serde::Deserialize;

/// A picture file's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// Portable Network Graphics, its first frame if it's animated.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF, its first frame.
    Gif,
    /// WebP, its first frame if it's animated.
    Webp,
}

/// How large a picture may be, from `spec/pictures.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Limits {
    /// The most pixels a picture may have, its width times its height.
    pub most_pixels: u64,
}

/// Why a picture couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum PictureError {
    /// Bytes that aren't a PNG, JPEG, GIF or WebP file.
    #[error("not a picture xWorks reads: PNG, JPEG, GIF or WebP")]
    Unknown,
    /// A picture with more pixels than the limit.
    #[error("{width} by {height} pixels is more than the {most} a picture may have")]
    TooLarge {
        /// Its width.
        width: u64,
        /// Its height.
        height: u64,
        /// The most pixels a picture may have.
        most: u64,
    },
    /// A picture whose bytes are damaged or of a kind its decoder refuses.
    #[error("reading the {format:?} file: {reason}")]
    Damaged {
        /// Its format.
        format: Format,
        /// What its decoder said.
        reason: String,
    },
}

/// A picture decoded: its format, its size as it shows, which is its file's
/// turned as its orientation says, and its pixels in sRGB, scaled to fit
/// [`scene::MAX_SIDE`] if they're larger.
#[derive(Clone, Debug)]
pub struct Picture {
    /// The file's format.
    pub format: Format,
    /// Its width as it shows, in its file's pixels.
    pub width: u32,
    /// Its height as it shows, in its file's pixels.
    pub height: u32,
    /// Its pixels, ready to draw.
    pub image: scene::Image,
}

/// The format of the picture file `bytes` begins, from its signature.
pub fn format(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else {
        None
    }
}

/// The picture file `bytes`, of whichever format it is, decoded.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Picture, PictureError> {
    decode_as(format(bytes).ok_or(PictureError::Unknown)?, bytes, limits)
}

/// The picture file `bytes` decoded as `format`.
pub fn decode_as(format: Format, bytes: &[u8], limits: &Limits) -> Result<Picture, PictureError> {
    let damaged = |reason: String| PictureError::Damaged { format, reason };
    let raw = match format {
        Format::Png => png(bytes, limits).map_err(damaged_from(format))?,
        Format::Jpeg => jpeg(bytes, limits).map_err(damaged_from(format))?,
        Format::Gif => gif(bytes, limits).map_err(damaged_from(format))?,
        Format::Webp => webp(bytes, limits).map_err(damaged_from(format))?,
    };
    let Raw {
        width,
        height,
        mut rgba,
        icc,
        orientation,
        colour,
    } = raw;
    // An sRGB JPEG, shown as it's stored, keeps its file.
    let keep =
        format == Format::Jpeg && colour && orientation == 1 && icc.as_deref().is_none_or(is_srgb);
    if let Some(icc) = icc {
        to_srgb(&mut rgba, &icc);
    }
    let (width, height, rgba) = orient(width, height, rgba, orientation);
    let image = if keep {
        scene::Image::with_jpeg(width, height, rgba, Arc::new(bytes.to_vec()))
    } else {
        scene::Image::new(width, height, rgba)
    }
    .map_err(|e| damaged(e.to_string()))?;
    Ok(Picture {
        format,
        width,
        height,
        image,
    })
}

/// What a decoder gives: straight RGBA, row by row, and what to do with it.
struct Raw {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    icc: Option<Vec<u8>>,
    /// The EXIF orientation, 1 to 8; 1 shows it as it's stored.
    orientation: u8,
    /// Whether it's stored in colour, as RGB or YCbCr: not grey, and not
    /// CMYK.
    colour: bool,
}

/// Why a decoder failed: a picture too large, or what it said.
enum Failure {
    TooLarge(u64, u64, u64),
    Said(String),
}

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(e: E) -> Failure {
        Failure::Said(e.to_string())
    }
}

fn damaged_from(format: Format) -> impl Fn(Failure) -> PictureError {
    move |failure| match failure {
        Failure::TooLarge(width, height, most) => PictureError::TooLarge {
            width,
            height,
            most,
        },
        Failure::Said(reason) => PictureError::Damaged { format, reason },
    }
}

/// Refuses a picture `width` by `height` with more pixels than `limits`
/// allow, or none.
fn check(width: u64, height: u64, limits: &Limits) -> Result<usize, Failure> {
    let pixels = width.saturating_mul(height);
    if pixels > limits.most_pixels {
        return Err(Failure::TooLarge(width, height, limits.most_pixels));
    }
    if pixels == 0 {
        return Err(Failure::Said(format!("{width} by {height} pixels")));
    }
    usize::try_from(pixels * 4).map_err(|_| Failure::TooLarge(width, height, limits.most_pixels))
}

fn png(bytes: &[u8], limits: &Limits) -> Result<Raw, Failure> {
    // The size, from the header that comes first, before the decoder
    // reads further.
    if let Some(ihdr) = bytes.get(12..24).filter(|h| h.starts_with(b"IHDR")) {
        let side =
            |at: usize| u32::from_be_bytes([ihdr[at], ihdr[at + 1], ihdr[at + 2], ihdr[at + 3]]);
        check(u64::from(side(4)), u64::from(side(8)), limits)?;
    }
    // Room for the image at 16 bits a channel, and its rows on the way.
    let budget = usize::try_from(limits.most_pixels.saturating_mul(8)).unwrap_or(usize::MAX);
    let mut decoder =
        ::png::Decoder::new_with_limits(Cursor::new(bytes), ::png::Limits { bytes: budget });
    decoder.set_transformations(::png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let (width, height) = (reader.info().width, reader.info().height);
    check(u64::from(width), u64::from(height), limits)?;
    let icc = reader.info().icc_profile.as_ref().map(|p| p.to_vec());
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| Failure::Said("too large".to_string()))?;
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer)?;
    let channels = match frame.color_type {
        ::png::ColorType::Grayscale => 1,
        ::png::ColorType::GrayscaleAlpha => 2,
        ::png::ColorType::Rgb => 3,
        ::png::ColorType::Rgba => 4,
        ::png::ColorType::Indexed => return Err(Failure::Said("palette left".to_string())),
    };
    if frame.bit_depth != ::png::BitDepth::Eight {
        return Err(Failure::Said(format!("{:?} bits", frame.bit_depth)));
    }
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in buffer[..frame.buffer_size()]
        .chunks_exact(frame.line_size)
        .take(height as usize)
    {
        expand(&row[..width as usize * channels], channels, &mut rgba);
    }
    Ok(Raw {
        width,
        height,
        rgba,
        icc,
        orientation: 1,
        colour: true,
    })
}

fn jpeg(bytes: &[u8], limits: &Limits) -> Result<Raw, Failure> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    // The limit is on pixels, not on sides: let the decoder take any a
    // JPEG can have.
    let options = DecoderOptions::default()
        .set_max_width(usize::from(u16::MAX))
        .set_max_height(usize::from(u16::MAX))
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers()?;
    let info = decoder
        .info()
        .ok_or_else(|| Failure::Said("no headers".to_string()))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    check(u64::from(width), u64::from(height), limits)?;
    // The profile of a CMYK picture is left: the decoder makes RGB of it
    // its own way.
    let cmyk = matches!(
        decoder.input_colorspace(),
        Some(ColorSpace::CMYK | ColorSpace::YCCK)
    );
    let icc = decoder.icc_profile().filter(|_| !cmyk);
    let colour = matches!(
        decoder.input_colorspace(),
        Some(ColorSpace::YCbCr | ColorSpace::RGB)
    );
    let orientation = decoder.exif().map_or(1, |exif| orientation(exif));
    let pixels = decoder.decode()?;
    let channels = match decoder.output_colorspace() {
        Some(ColorSpace::RGBA) => 4,
        Some(ColorSpace::RGB) => 3,
        Some(ColorSpace::LumaA) => 2,
        Some(ColorSpace::Luma) => 1,
        other => return Err(Failure::Said(format!("decoded as {other:?}"))),
    };
    let wanted = width as usize * height as usize * channels;
    if pixels.len() < wanted {
        return Err(Failure::Said("too few pixels".to_string()));
    }
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    expand(&pixels[..wanted], channels, &mut rgba);
    Ok(Raw {
        width,
        height,
        rgba,
        icc,
        orientation,
        colour,
    })
}

fn gif(bytes: &[u8], limits: &Limits) -> Result<Raw, Failure> {
    let mut options = ::gif::DecodeOptions::new();
    options.set_color_output(::gif::ColorOutput::RGBA);
    let budget = NonZeroU64::new(limits.most_pixels.saturating_mul(4)).unwrap_or(NonZeroU64::MIN);
    options.set_memory_limit(::gif::MemoryLimit::Bytes(budget));
    let mut decoder = options.read_info(Cursor::new(bytes))?;
    let (screen_w, screen_h) = (u32::from(decoder.width()), u32::from(decoder.height()));
    let icc = decoder.icc_profile().map(<[u8]>::to_vec);
    if screen_w > 0 && screen_h > 0 {
        check(u64::from(screen_w), u64::from(screen_h), limits)?;
    }
    let frame = decoder
        .read_next_frame()?
        .ok_or_else(|| Failure::Said("no frames".to_string()))?;
    // A screen of no size takes its first frame's.
    let (width, height) = if screen_w == 0 || screen_h == 0 {
        (
            u32::from(frame.left) + u32::from(frame.width),
            u32::from(frame.top) + u32::from(frame.height),
        )
    } else {
        (screen_w, screen_h)
    };
    let size = check(u64::from(width), u64::from(height), limits)?;
    let mut rgba = vec![0; size];
    let (fw, fh) = (usize::from(frame.width), usize::from(frame.height));
    // As much of each of the frame's rows as falls on the screen, which is
    // none if the frame starts right of it.
    let left = usize::from(frame.left);
    let across = fw.min((width as usize).saturating_sub(left));
    if across > 0 && frame.buffer.len() >= fw * fh * 4 {
        for y in 0..fh {
            let to_y = usize::from(frame.top) + y;
            if to_y >= height as usize {
                break;
            }
            let from = y * fw * 4;
            let to = (to_y * width as usize + left) * 4;
            rgba[to..to + across * 4].copy_from_slice(&frame.buffer[from..from + across * 4]);
        }
    }
    Ok(Raw {
        width,
        height,
        rgba,
        icc,
        orientation: 1,
        colour: true,
    })
}

fn webp(bytes: &[u8], limits: &Limits) -> Result<Raw, Failure> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))?;
    let (width, height) = decoder.dimensions();
    check(u64::from(width), u64::from(height), limits)?;
    decoder.set_memory_limit(
        usize::try_from(limits.most_pixels.saturating_mul(8)).unwrap_or(usize::MAX),
    );
    let icc = decoder.icc_profile().ok().flatten();
    let size = decoder
        .output_buffer_size()
        .ok_or_else(|| Failure::Said("too large".to_string()))?;
    let mut buffer = vec![0; size];
    decoder.read_image(&mut buffer)?;
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    expand(&buffer, channels, &mut rgba);
    Ok(Raw {
        width,
        height,
        rgba,
        icc,
        orientation: 1,
        colour: true,
    })
}

/// Pixels of `channels` channels (grey, grey and alpha, RGB or RGBA)
/// added to `out` as RGBA.
fn expand(pixels: &[u8], channels: usize, out: &mut Vec<u8>) {
    match channels {
        4 => out.extend_from_slice(pixels),
        3 => {
            for p in pixels.as_chunks::<3>().0 {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        2 => {
            for p in pixels.as_chunks::<2>().0 {
                out.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        _ => {
            for &g in pixels {
                out.extend_from_slice(&[g, g, g, 255]);
            }
        }
    }
}

/// `rgba` converted in place from the colour space of the ICC profile
/// `icc` to sRGB: from an RGB profile, or a grey one for a grey picture.
/// A profile that can't be read or used leaves it as it is.
fn to_srgb(rgba: &mut [u8], icc: &[u8]) {
    use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
    let Ok(profile) = ColorProfile::new_from_slice(icc) else {
        return;
    };
    let srgb = ColorProfile::new_srgb();
    let options = TransformOptions::default();
    // In pieces, so the copy alongside stays small.
    const PIECE: usize = 1 << 16;
    match profile.color_space {
        DataColorSpace::Rgb => {
            let Ok(transform) =
                profile.create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options)
            else {
                return;
            };
            let mut out = vec![0; PIECE * 4];
            for piece in rgba.chunks_mut(PIECE * 4) {
                let out = &mut out[..piece.len()];
                if transform.transform(piece, out).is_ok() {
                    piece.copy_from_slice(out);
                }
            }
        }
        DataColorSpace::Gray => {
            let grey = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == p[1] && p[1] == p[2]);
            if !grey {
                return;
            }
            let Ok(transform) =
                profile.create_transform_8bit(Layout::GrayAlpha, &srgb, Layout::Rgba, options)
            else {
                return;
            };
            let mut src = vec![0; PIECE * 2];
            let mut out = vec![0; PIECE * 4];
            for piece in rgba.chunks_mut(PIECE * 4) {
                let n = piece.len() / 4;
                for (i, p) in piece.as_chunks::<4>().0.iter().enumerate() {
                    src[i * 2] = p[0];
                    src[i * 2 + 1] = p[3];
                }
                if transform
                    .transform(&src[..n * 2], &mut out[..n * 4])
                    .is_ok()
                {
                    piece.copy_from_slice(&out[..n * 4]);
                }
            }
        }
        _ => {}
    }
}

/// Whether the ICC profile `icc` is sRGB's, as near as 8 bits tell: an RGB
/// profile whose conversion to sRGB leaves every colour of a grid of six
/// levels a channel within a level of itself. One that can't be read
/// isn't.
fn is_srgb(icc: &[u8]) -> bool {
    use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
    let Ok(profile) = ColorProfile::new_from_slice(icc) else {
        return false;
    };
    if profile.color_space != DataColorSpace::Rgb {
        return false;
    }
    let srgb = ColorProfile::new_srgb();
    let Ok(transform) = profile.create_transform_8bit(
        Layout::Rgba,
        &srgb,
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return false;
    };
    let steps = [0u8, 51, 102, 153, 204, 255];
    let mut grid = Vec::with_capacity(steps.len().pow(3) * 4);
    for r in steps {
        for g in steps {
            for b in steps {
                grid.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    let mut out = vec![0; grid.len()];
    transform.transform(&grid, &mut out).is_ok()
        && grid.iter().zip(&out).all(|(a, b)| a.abs_diff(*b) <= 1)
}

/// The orientation in EXIF data that starts at its TIFF header: tag 274
/// of its first directory, 1 to 8; 1 if it says none.
fn orientation(exif: &[u8]) -> u8 {
    let little = match exif.get(0..4) {
        Some(b"II*\0") => true,
        Some(b"MM\0*") => false,
        _ => return 1,
    };
    let u16_at = |at: usize| {
        let b: [u8; 2] = exif.get(at..at + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| {
        let b: [u8; 4] = exif.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let found = (|| {
        let directory = u32_at(4)? as usize;
        let entries = u16_at(directory)? as usize;
        (0..entries).find_map(|i| {
            let entry = directory + 2 + 12 * i;
            // A SHORT, its value in the first two bytes of the field.
            (u16_at(entry)? == 274 && u16_at(entry + 2)? == 3).then(|| u16_at(entry + 8))?
        })
    })();
    match found {
        Some(o @ 1..=8) => o as u8,
        _ => 1,
    }
}

/// `rgba`, `width` by `height`, turned and flipped as EXIF orientation
/// `orientation` says it's to be shown; with its new size.
fn orient(width: u32, height: u32, rgba: Vec<u8>, orientation: u8) -> (u32, u32, Vec<u8>) {
    if !(2..=8).contains(&orientation) {
        return (width, height, rgba);
    }
    let (w, h) = (width as usize, height as usize);
    let turned = orientation >= 5;
    let (ow, oh) = if turned { (h, w) } else { (w, h) };
    let mut out = vec![0; rgba.len()];
    for y in 0..oh {
        for x in 0..ow {
            // Where the pixel shown at (x, y) is stored.
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let (from, to) = ((sy * w + sx) * 4, (y * ow + x) * 4);
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    (ow as u32, oh as u32, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_known_by_their_signatures() {
        assert_eq!(format(b"\x89PNG\r\n\x1a\nrest"), Some(Format::Png));
        assert_eq!(format(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Format::Jpeg));
        assert_eq!(format(b"GIF89a"), Some(Format::Gif));
        assert_eq!(format(b"RIFF\0\0\0\0WEBPVP8 "), Some(Format::Webp));
        assert_eq!(format(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(format(b"%PDF-1.7"), None);
    }

    #[test]
    fn each_orientation_turns_and_flips_as_exif_says() {
        // Stored 3 by 2: 1 2 3 over 4 5 6, each pixel's red its number.
        let stored: Vec<u8> = (1..=6).flat_map(|v| [v, 0, 0, 255]).collect();
        let shown = |o: u8| {
            let (w, h, px) = orient(3, 2, stored.clone(), o);
            let reds: Vec<u8> = px.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
            (w, h, reds)
        };
        assert_eq!(shown(1), (3, 2, vec![1, 2, 3, 4, 5, 6]));
        assert_eq!(shown(2), (3, 2, vec![3, 2, 1, 6, 5, 4]), "mirrored");
        assert_eq!(shown(3), (3, 2, vec![6, 5, 4, 3, 2, 1]), "half a turn");
        assert_eq!(shown(4), (3, 2, vec![4, 5, 6, 1, 2, 3]), "upside down");
        assert_eq!(shown(5), (2, 3, vec![1, 4, 2, 5, 3, 6]), "transposed");
        assert_eq!(
            shown(6),
            (2, 3, vec![4, 1, 5, 2, 6, 3]),
            "a quarter turn right"
        );
        assert_eq!(shown(7), (2, 3, vec![6, 3, 5, 2, 4, 1]), "transversed");
        assert_eq!(
            shown(8),
            (2, 3, vec![3, 6, 2, 5, 1, 4]),
            "a quarter turn left"
        );
    }

    #[test]
    fn exif_orientation_is_read_in_either_byte_order() {
        let little = [
            b"II*\0".as_slice(),
            &8u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &274u16.to_le_bytes(),
            &3u16.to_le_bytes(),
            &1u32.to_le_bytes(),
            &6u16.to_le_bytes(),
            &[0, 0],
        ]
        .concat();
        assert_eq!(orientation(&little), 6);
        let big = [
            b"MM\0*".as_slice(),
            &8u32.to_be_bytes(),
            &1u16.to_be_bytes(),
            &274u16.to_be_bytes(),
            &3u16.to_be_bytes(),
            &1u32.to_be_bytes(),
            &8u16.to_be_bytes(),
            &[0, 0],
        ]
        .concat();
        assert_eq!(orientation(&big), 8);
        assert_eq!(orientation(&big[..12]), 1, "cut short");
        assert_eq!(orientation(b"nonsense"), 1);
    }

    #[test]
    fn the_limit_is_on_pixels_and_holds_at_the_limit() {
        let limits = Limits { most_pixels: 100 };
        assert!(check(10, 10, &limits).is_ok());
        assert!(matches!(
            check(10, 11, &limits),
            Err(Failure::TooLarge(10, 11, 100))
        ));
        assert!(matches!(check(0, 10, &limits), Err(Failure::Said(_))));
        assert!(matches!(
            check(u64::MAX, 2, &limits),
            Err(Failure::TooLarge(..))
        ));
    }
}
