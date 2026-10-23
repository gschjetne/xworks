//! Pictures read from files (docs/ROADMAP.md, M2 stage 4; project owner,
//! review pysc65hp): PNG, JPEG, GIF and WebP, a GIF's first frame, in
//! sRGB, turned as their EXIF says, and refused when they're too large or
//! damaged.
//!
//! The PNG, GIF and WebP pictures are made here with the formats' own
//! encoders. The JPEGs in `tests/pictures/` were made with Pillow, from
//! the same gradient: `gradient.jpg` as it is, `turned.jpg` stored a
//! quarter turn left with EXIF orientation 6, and `grey.jpg` a grey ramp.

use std::borrow::Cow;
use std::path::Path;

use app::picture::{self, Format, Limits, PictureError};

fn limits() -> Limits {
    let path = testkit::workspace_root().join("spec/pictures.ron");
    ron::from_str(&std::fs::read_to_string(path).expect("spec/pictures.ron")).expect("valid")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/pictures")
            .join(name),
    )
    .expect("a test picture")
}

/// The picture's pixels, straight, with alpha.
fn rgba(picture: &picture::Picture) -> Vec<u8> {
    let (rgb, alpha) = picture.image.straight();
    rgb.as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .flat_map(|(i, p)| [p[0], p[1], p[2], alpha.as_ref().map_or(255, |a| a[i])])
        .collect()
}

/// RGBA bytes as pixels.
fn pixels(rgba: &[u8]) -> Vec<[u8; 4]> {
    rgba.as_chunks::<4>().0.to_vec()
}

fn pixel(picture: &picture::Picture, x: u32, y: u32) -> [u8; 4] {
    let px = rgba(picture);
    let i = ((y * picture.width + x) * 4) as usize;
    px[i..i + 4].try_into().unwrap()
}

fn close(a: [u8; 4], b: [u8; 4], by: u8) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= by)
}

/// A PNG of `data`, `width` by `height`, in `color` at 8 bits, with
/// `palette` and `icc` if given.
fn png(
    width: u32,
    height: u32,
    color: png::ColorType,
    data: &[u8],
    palette: Option<&[u8]>,
    icc: Option<Vec<u8>>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut info = png::Info::with_size(width, height);
    info.color_type = color;
    info.bit_depth = png::BitDepth::Eight;
    info.palette = palette.map(|p| Cow::Owned(p.to_vec()));
    info.icc_profile = icc.map(Cow::Owned);
    let encoder = png::Encoder::with_info(&mut out, info).expect("an encoder");
    let mut writer = encoder.write_header().expect("a header");
    writer.write_image_data(data).expect("the pixels");
    writer.finish().expect("the end");
    out
}

/// 4 by 3 pixels, each of its own colour, some half transparent.
fn colours() -> Vec<u8> {
    (0..12u8)
        .flat_map(|i| {
            [
                i * 20,
                255 - i * 20,
                100 + i,
                if i % 4 == 3 { 128 } else { 255 },
            ]
        })
        .collect()
}

#[test]
fn png_reads_in_every_colour_type() {
    let limits = limits();
    let rgba_data = colours();
    let decoded = picture::decode(
        &png(4, 3, png::ColorType::Rgba, &rgba_data, None, None),
        &limits,
    )
    .expect("RGBA");
    assert_eq!(
        (decoded.format, decoded.width, decoded.height),
        (Format::Png, 4, 3)
    );
    for (i, (got, want)) in pixels(&rgba(&decoded))
        .into_iter()
        .zip(pixels(&rgba_data))
        .enumerate()
    {
        assert!(close(got, want, 1), "pixel {i}: {got:?} for {want:?}");
    }
    let grey = picture::decode(
        &png(2, 1, png::ColorType::Grayscale, &[0, 200], None, None),
        &limits,
    )
    .expect("grey");
    assert_eq!(rgba(&grey), [0, 0, 0, 255, 200, 200, 200, 255]);
    let indexed = picture::decode(
        &png(
            2,
            1,
            png::ColorType::Indexed,
            &[1, 0],
            Some(&[10, 20, 30, 40, 50, 60]),
            None,
        ),
        &limits,
    )
    .expect("indexed");
    assert_eq!(rgba(&indexed), [40, 50, 60, 255, 10, 20, 30, 255]);
}

#[test]
fn a_gif_shows_its_first_frame_on_its_screen() {
    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, 4, 2, &[]).expect("an encoder");
        // The first frame covers the right half of the screen; the second,
        // all of it, isn't shown.
        let mut first: Vec<u8> = [[255, 0, 0, 255], [0, 0, 255, 255]].concat().repeat(2);
        let mut frame = gif::Frame::from_rgba(2, 2, &mut first);
        frame.left = 2;
        encoder.write_frame(&frame).expect("a frame");
        let mut second = [0, 255, 0, 255].repeat(8);
        encoder
            .write_frame(&gif::Frame::from_rgba(4, 2, &mut second))
            .expect("a frame");
    }
    let decoded = picture::decode(&out, &limits()).expect("a GIF");
    assert_eq!(
        (decoded.format, decoded.width, decoded.height),
        (Format::Gif, 4, 2)
    );
    assert_eq!(pixel(&decoded, 0, 0)[3], 0, "outside the frame, nothing");
    assert_eq!(pixel(&decoded, 2, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&decoded, 3, 1), [0, 0, 255, 255]);
    // A first frame that starts right of the screen shows nothing.
    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, 4, 2, &[]).expect("an encoder");
        let mut pixels = [255, 0, 0, 255].repeat(4);
        let mut frame = gif::Frame::from_rgba(2, 2, &mut pixels);
        frame.left = 9;
        encoder.write_frame(&frame).expect("a frame");
    }
    let decoded = picture::decode(&out, &limits()).expect("a GIF");
    assert!(rgba(&decoded).iter().all(|&b| b == 0), "a clear screen");
}

#[test]
fn webp_reads_losslessly_with_alpha() {
    let data = colours();
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(&data, 4, 3, image_webp::ColorType::Rgba8)
        .expect("a WebP");
    let decoded = picture::decode(&out, &limits()).expect("it reads");
    assert_eq!(
        (decoded.format, decoded.width, decoded.height),
        (Format::Webp, 4, 3)
    );
    for (got, want) in pixels(&rgba(&decoded)).into_iter().zip(pixels(&data)) {
        assert!(close(got, want, 1), "{got:?} for {want:?}");
    }
}

#[test]
fn jpeg_reads_and_turns_as_its_exif_says() {
    let limits = limits();
    let upright = picture::decode(&fixture("gradient.jpg"), &limits).expect("a JPEG");
    assert_eq!(
        (upright.format, upright.width, upright.height),
        (Format::Jpeg, 32, 24)
    );
    // Red grows across, green down; blue is 64 throughout.
    for (x, y, want) in [
        (0, 0, [0, 0, 64, 255]),
        (31, 0, [255, 0, 64, 255]),
        (0, 23, [0, 255, 64, 255]),
        (31, 23, [255, 255, 64, 255]),
    ] {
        let got = pixel(&upright, x, y);
        assert!(close(got, want, 8), "({x}, {y}): {got:?} for {want:?}");
    }
    // Stored on its side, it shows upright as the other does.
    let turned = picture::decode(&fixture("turned.jpg"), &limits).expect("a JPEG");
    assert_eq!((turned.width, turned.height), (32, 24));
    let (a, b) = (rgba(&upright), rgba(&turned));
    let worst = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 8, "{worst} levels from the upright one");
    // A grey JPEG is grey.
    let grey = picture::decode(&fixture("grey.jpg"), &limits).expect("a JPEG");
    assert_eq!((grey.width, grey.height), (16, 16));
    let [r, g, b, a] = pixel(&grey, 15, 8);
    assert!(r == g && g == b && r > 220 && a == 255, "{r} {g} {b} {a}");
}

#[test]
fn colour_is_converted_to_srgb_from_the_pictures_profile() {
    let p3 = moxcms::ColorProfile::new_display_p3()
        .encode()
        .expect("a P3 profile");
    let data = [[200, 150, 100, 255], [128, 128, 128, 255]].concat();
    let tagged = png(2, 1, png::ColorType::Rgba, &data, None, Some(p3.clone()));
    let decoded = picture::decode(&tagged, &limits()).expect("it reads");
    // Display P3's (200, 150, 100) is sRGB's (209, 147, 91), worked out by
    // the two spaces' matrices and curves; grey stays grey, as both
    // spaces have the same white and the same curve.
    let got = pixel(&decoded, 0, 0);
    assert!(close(got, [209, 147, 91, 255], 3), "{got:?}");
    let got = pixel(&decoded, 1, 0);
    assert!(close(got, [128, 128, 128, 255], 1), "{got:?}");
    // The same in WebP; and untagged, nothing changes.
    let mut out = Vec::new();
    let mut encoder = image_webp::WebPEncoder::new(&mut out);
    encoder.set_icc_profile(p3);
    encoder
        .encode(&data, 2, 1, image_webp::ColorType::Rgba8)
        .expect("a WebP");
    let got = pixel(&picture::decode(&out, &limits()).expect("it reads"), 0, 0);
    assert!(close(got, [209, 147, 91, 255], 3), "{got:?}");
    let plain = png(2, 1, png::ColorType::Rgba, &data, None, None);
    let got = pixel(&picture::decode(&plain, &limits()).expect("it reads"), 0, 0);
    assert_eq!(got, [200, 150, 100, 255]);
    // A profile that can't be read is taken as sRGB.
    let broken = png(
        2,
        1,
        png::ColorType::Rgba,
        &data,
        None,
        Some(b"not a profile".to_vec()),
    );
    let got = pixel(
        &picture::decode(&broken, &limits()).expect("it reads"),
        0,
        0,
    );
    assert_eq!(got, [200, 150, 100, 255]);
}

#[test]
fn pictures_larger_than_the_limit_are_refused_from_their_headers() {
    let small = Limits { most_pixels: 12 };
    let data = colours();
    assert!(picture::decode(&png(4, 3, png::ColorType::Rgba, &data, None, None), &small).is_ok());
    let more = [data.clone(), data[..16].to_vec()].concat();
    assert!(matches!(
        picture::decode(&png(4, 4, png::ColorType::Rgba, &more, None, None), &small),
        Err(PictureError::TooLarge {
            width: 4,
            height: 4,
            most: 12
        })
    ));
    // A header claiming more than the limit, with no pixels behind it, is
    // refused before anything is decoded.
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 20_000, 20_000);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let writer = encoder.write_header().expect("a header");
        drop(writer);
    }
    assert!(matches!(
        picture::decode(&out, &limits()),
        Err(PictureError::TooLarge { .. })
    ));
}

#[test]
fn damaged_pictures_are_errors() {
    let limits = limits();
    assert!(matches!(
        picture::decode(b"no picture at all", &limits),
        Err(PictureError::Unknown)
    ));
    let whole = [
        png(4, 3, png::ColorType::Rgba, &colours(), None, None),
        fixture("gradient.jpg"),
    ];
    for file in whole {
        for cut in [9, 20, file.len() / 2, file.len() - 3] {
            let _ = picture::decode(&file[..cut], &limits);
        }
        let mut flipped = file.clone();
        for i in (40..flipped.len()).step_by(7) {
            flipped[i] ^= 0x5a;
        }
        let _ = picture::decode(&flipped, &limits);
    }
    assert!(matches!(
        picture::decode_as(Format::Gif, b"GIF89a\x01", &limits),
        Err(PictureError::Damaged {
            format: Format::Gif,
            ..
        })
    ));
    assert!(picture::decode_as(Format::Webp, b"RIFF\0\0\0\0WEBPVP8 ", &limits).is_err());
}

/// `jpeg` with the ICC profile `icc` in an APP2 segment after its JFIF
/// header, as cameras and editors embed one.
fn with_icc(jpeg: &[u8], icc: &[u8]) -> Vec<u8> {
    // SOI, then APP0: its marker and its length, which counts itself.
    let app0 = 4 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]]));
    let mut segment = vec![0xFF, 0xE2];
    let len = (2 + 12 + 2 + icc.len()) as u16;
    segment.extend_from_slice(&len.to_be_bytes());
    segment.extend_from_slice(b"ICC_PROFILE\0");
    segment.extend_from_slice(&[1, 1]);
    segment.extend_from_slice(icc);
    [&jpeg[..app0], &segment, &jpeg[app0..]].concat()
}

/// An sRGB JPEG shown as it's stored keeps its file, for PDF to hold
/// unchanged (project owner, review dcn5ejrd); any other picture is its
/// pixels alone.
#[test]
fn an_srgb_jpeg_keeps_its_file() {
    let limits = limits();
    let gradient = fixture("gradient.jpg");
    let decoded = picture::decode(&gradient, &limits).expect("a JPEG");
    assert_eq!(decoded.image.jpeg(), Some(gradient.as_slice()));
    // Tagged sRGB, it's still sRGB.
    let srgb = moxcms::ColorProfile::new_srgb()
        .encode()
        .expect("an sRGB profile");
    let tagged = with_icc(&gradient, &srgb);
    let decoded = picture::decode(&tagged, &limits).expect("a JPEG");
    assert_eq!(decoded.image.jpeg(), Some(tagged.as_slice()));
    // Tagged otherwise, it's converted, and drawn and printed from its
    // pixels.
    let p3 = moxcms::ColorProfile::new_display_p3()
        .encode()
        .expect("a P3 profile");
    let decoded = picture::decode(&with_icc(&gradient, &p3), &limits).expect("a JPEG");
    assert!(decoded.image.jpeg().is_none());
    let decoded = picture::decode(&with_icc(&gradient, b"not a profile"), &limits).expect("a JPEG");
    assert!(
        decoded.image.jpeg().is_none(),
        "a profile that can't be read"
    );
    // Turned by its EXIF, it isn't shown as it's stored; grey isn't sRGB.
    for name in ["turned.jpg", "grey.jpg"] {
        let decoded = picture::decode(&fixture(name), &limits).expect("a JPEG");
        assert!(decoded.image.jpeg().is_none(), "{name}");
    }
    let png = png(4, 3, png::ColorType::Rgba, &colours(), None, None);
    assert!(
        picture::decode(&png, &limits)
            .expect("a PNG")
            .image
            .jpeg()
            .is_none()
    );
}
