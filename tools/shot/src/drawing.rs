//! M2's drawing (docs/ROADMAP.md, M2 stage 4): images, clips and
//! transforms, underlines and lines through text. The pictures are made
//! here, in integers, and the photograph goes through a PNG file and the
//! app's decoder, as a placed picture does; JPEG's decoder isn't used, as
//! its SIMD can't be turned off and might draw other pixels on Arm.

use std::sync::OnceLock;

use scene::{Color, Group, Image, Item, Page, Path, Point, Rect, Segment, Transform};
use text::units::{INCH, POINT};
use text::{Align, Decoration, Language, Paragraph, Span, Style};

use crate::pages::fonts;

/// A landscape, `width` by `height`, worked out in integers: a sky
/// darkening upwards, a sun, two hills and a striped field.
fn landscape(width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (i64::from(width), i64::from(height));
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            // The hills: parabolas, their tops at a third and two thirds.
            let hill = |cx: i64, top: i64, spread: i64| {
                let dx = x - cx;
                top + dx * dx * h / (spread * spread)
            };
            let near = hill(w * 2 / 3, h / 2, w / 2);
            let far = hill(w / 4, h * 2 / 5, w / 3);
            let (sx, sy, sr) = (w * 3 / 4, h / 4, h / 9);
            let sun = (x - sx) * (x - sx) + (y - sy) * (y - sy) <= sr * sr;
            let colour = if y >= near {
                // The field, its furrows in bands.
                let band = ((y - near) * 8 / (h - near).max(1)) % 2;
                if band == 0 {
                    [70, 140 + (y * 40 / h) as u8, 50]
                } else {
                    [100, 160, 60]
                }
            } else if y >= far {
                [60, 110, 90]
            } else if sun {
                [250, 210, 90]
            } else {
                let t = (y * 255 / h) as u8;
                [60 + t / 3, 110 + t / 3, 200 + t / 5]
            };
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }
    rgba
}

/// The landscape as a PNG file, read by the app's decoder.
fn photograph() -> &'static Image {
    static PHOTO: OnceLock<Image> = OnceLock::new();
    PHOTO.get_or_init(|| {
        let (width, height) = (1024, 640);
        let mut file = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut file, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("a PNG header");
            writer
                .write_image_data(&landscape(width, height))
                .expect("the PNG's pixels");
        }
        let limits: app::picture::Limits = ron::from_str(
            &std::fs::read_to_string(testkit::workspace_root().join("spec/pictures.ron"))
                .expect("spec/pictures.ron"),
        )
        .expect("valid");
        app::picture::decode(&file, &limits)
            .expect("the PNG decodes")
            .image
    })
}

/// A heart, 9 by 8 pixels, its edge half transparent: drawn large, its
/// pixels blend into each other, and its edges hold.
fn icon() -> Image {
    const ROWS: [&str; 8] = [
        ".##...##.",
        "#oo#.#oo#",
        "#ooo#ooo#",
        "#ooooooo#",
        ".#ooooo#.",
        "..#ooo#..",
        "...#o#...",
        "....#....",
    ];
    let rgba = ROWS
        .iter()
        .flat_map(|row| row.chars())
        .flat_map(|c| match c {
            '#' => [150, 20, 40, 160],
            'o' => [230, 50, 70, 255],
            _ => [0, 0, 0, 0],
        })
        .collect();
    Image::new(9, 8, rgba).expect("an icon")
}

/// A ramp from nearly opaque to clear, left to right, in one colour.
fn veil() -> Image {
    let rgba = (0..64u32)
        .flat_map(|x| [30, 60, 160, (220 - x * 220 / 63) as u8])
        .collect::<Vec<u8>>()
        .repeat(4);
    Image::new(64, 4, rgba).expect("a veil")
}

/// `text` in `family`, `size` points, laid out `width` wide at `at`.
fn line(text: &str, family: &str, size: i64, at: Point, width: i64) -> Vec<Item> {
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: family.to_string(),
            size: size * POINT,
            weight: 400,
            italic: false,
            language: Language::und(),
        },
    }];
    let layout = text::layout(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width,
        },
    );
    layout.paint(fonts(), at, None, |_| Color::BLACK)
}

fn image(image: &Image, x: i64, y: i64, width: i64, height: i64) -> Item {
    Item::Image {
        image: image.clone(),
        rect: Rect::new(x, y, width, height),
        alt: None,
        source: None,
    }
}

/// Images, 6 by 4 inches at 96 pixels per inch: a photograph large and in
/// two smaller sizes, from its smaller levels; a small icon drawn large;
/// and a transparent ramp over text.
pub(crate) fn images_page() -> Page {
    let mut page = Page::new(6 * INCH, 4 * INCH);
    page.fill_background(Color::WHITE);
    let photo = photograph();
    let q = INCH / 4;
    page.items.push(image(photo, q, q, 3 * INCH, 15 * INCH / 8));
    page.items.push(image(photo, 13 * q, q, INCH, 5 * INCH / 8));
    page.items.push(image(
        photo,
        13 * q,
        q + 3 * INCH / 4,
        INCH / 2,
        5 * INCH / 16,
    ));
    page.items.extend(line(
        "A photograph, its smaller sizes and an icon",
        "Noto Sans",
        9,
        Point::new(q, 9 * q),
        3 * INCH,
    ));
    page.items.push(Item::Fill {
        path: Path::rect(19 * q, q, INCH + q / 2, INCH + q / 2),
        color: Color::rgb(235, 238, 225),
    });
    page.items
        .push(image(&icon(), 19 * q + q / 4, q + q / 4, INCH + 1, INCH));
    page.items.extend(line(
        "Text under a veil that clears to the right.",
        "Noto Serif",
        14,
        Point::new(q, 11 * q),
        5 * INCH,
    ));
    page.items
        .push(image(&veil(), q, 11 * q, 5 * INCH, INCH / 3));
    page
}

/// A circle of radius `r` about (`cx`, `cy`), from four cubic arcs.
fn circle(cx: i64, cy: i64, r: i64) -> Path {
    let kr = r * 552 / 1000;
    let c = |dx: i64, dy: i64| Point::new(cx + dx, cy + dy);
    Path(vec![
        Segment::MoveTo(c(r, 0)),
        Segment::CubicTo(c(r, kr), c(kr, r), c(0, r)),
        Segment::CubicTo(c(-kr, r), c(-r, kr), c(-r, 0)),
        Segment::CubicTo(c(-r, -kr), c(-kr, -r), c(0, -r)),
        Segment::CubicTo(c(kr, -r), c(r, -kr), c(r, 0)),
        Segment::Close,
    ])
}

/// Clips and transforms, 5 by 4 inches at 144 pixels per inch: text
/// turned, scaled and mirrored; the photograph in a round clip, turned;
/// and a star clipping stripes, by the nonzero rule.
pub(crate) fn transforms_page() -> Page {
    let mut page = Page::new(5 * INCH, 4 * INCH);
    page.fill_background(Color::WHITE);
    let words = || {
        line(
            "Turned and scaled",
            "Noto Serif",
            12,
            Point::new(0, 0),
            2 * INCH,
        )
    };
    // About 37° by a 3-4-5 triangle, whose sine and cosine are exact
    // everywhere.
    let turn = Transform([0.8, 0.6, -0.6, 0.8, 0.0, 0.0]);
    page.items.push(Item::Group(Group {
        clip: None,
        transform: turn.then(Transform::translate(INCH / 2, INCH / 4)),
        items: words(),
    }));
    page.items.push(Item::Group(Group {
        clip: None,
        transform: Transform::scale(2.0, 1.5).then(Transform::translate(INCH / 4, 2 * INCH)),
        items: words(),
    }));
    // Mirrored, as in a looking glass.
    page.items.push(Item::Group(Group {
        clip: None,
        transform: Transform::scale(-1.0, 1.0).then(Transform::translate(9 * INCH / 4, 3 * INCH)),
        items: words(),
    }));
    // A quarter turn, down the right edge.
    page.items.push(Item::Group(Group {
        clip: None,
        transform: Transform::rotate(90.0).then(Transform::translate(19 * INCH / 4, INCH / 4)),
        items: line("Down the edge", "Noto Sans", 10, Point::new(0, 0), 3 * INCH),
    }));
    // The photograph, turned, seen through a circle.
    let (cx, cy, r) = (3 * INCH + INCH / 4, INCH + INCH / 8, 3 * INCH / 4);
    page.items.push(Item::Group(Group {
        clip: Some(circle(cx, cy, r)),
        transform: turn.then(Transform::translate(cx - INCH / 4, cy - INCH)),
        items: vec![image(photograph(), 0, 0, 2 * INCH, 5 * INCH / 4)],
    }));
    page.items.push(Item::Stroke {
        path: circle(cx, cy, r),
        width: POINT,
        color: Color::rgb(60, 60, 60),
    });
    // A five-pointed star drawn in one stroke: its middle winds twice,
    // and the nonzero rule fills it.
    let (sx, sy, sr) = (3 * INCH + INCH / 4, 3 * INCH + INCH / 8, 3 * INCH / 4);
    // The points of a pentagram, by the golden ratio, in thousandths.
    let star = [
        (0, -1000),
        (588, 809),
        (-951, -309),
        (951, -309),
        (-588, 809),
    ];
    let mut path: Vec<Segment> = star
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            let p = Point::new(sx + x * sr / 1000, sy + y * sr / 1000);
            if i == 0 {
                Segment::MoveTo(p)
            } else {
                Segment::LineTo(p)
            }
        })
        .collect();
    path.push(Segment::Close);
    let stripes: Vec<Item> = (0..12)
        .map(|i| Item::Fill {
            path: Path::rect(sx - sr + i * sr / 6, sy - sr, sr / 12, 2 * sr),
            color: if i % 2 == 0 {
                Color::rgb(40, 90, 170)
            } else {
                Color::rgb(220, 140, 30)
            },
        })
        .collect();
    page.items.push(Item::Group(Group {
        clip: Some(Path(path)),
        transform: Transform::IDENTITY,
        items: stripes,
    }));
    page
}

/// A paragraph of the decorations page: its stretches, each with its
/// text, lines and colour.
struct Decorated {
    family: &'static str,
    size: i64,
    language: &'static str,
    italic: bool,
    parts: &'static [(&'static str, Decoration, Color)],
}

const PLAIN: Decoration = Decoration::NONE;
const UNDER: Decoration = Decoration {
    underline: true,
    strikethrough: false,
};
const THROUGH: Decoration = Decoration {
    underline: false,
    strikethrough: true,
};
const BOTH: Decoration = Decoration {
    underline: true,
    strikethrough: true,
};
const INK: Color = Color::BLACK;
const RED: Color = Color::rgb(170, 30, 40);

const DECORATED: &[Decorated] = &[
    Decorated {
        family: "Noto Serif",
        size: 14,
        language: "en",
        italic: false,
        parts: &[
            ("Text ", PLAIN, INK),
            ("underlined", UNDER, INK),
            (", ", PLAIN, INK),
            ("struck through", THROUGH, RED),
            (" and ", PLAIN, INK),
            ("both at once", BOTH, INK),
            (".", PLAIN, INK),
        ],
    },
    Decorated {
        family: "Source Serif 4",
        size: 11,
        language: "en",
        italic: false,
        parts: &[(
            "Each font places its own lines: this underline wraps across the end of a line, and no space that hangs there is underlined.",
            UNDER,
            INK,
        )],
    },
    Decorated {
        family: "Inter",
        size: 24,
        language: "en",
        italic: false,
        parts: &[("Large ", UNDER, INK), ("and thick", BOTH, RED)],
    },
    Decorated {
        family: "Noto Sans",
        size: 11,
        language: "el",
        italic: false,
        parts: &[
            ("Ελληνικά ", UNDER, INK),
            ("και ", PLAIN, INK),
            ("Кириллица", THROUGH, INK),
        ],
    },
    Decorated {
        family: "Noto Serif",
        size: 12,
        language: "he",
        italic: true,
        parts: &[("שלום עולם, ", UNDER, INK), ("נטוי", BOTH, INK)],
    },
    Decorated {
        family: "Noto Serif",
        size: 12,
        language: "ar",
        italic: false,
        parts: &[("مرحبا ", UNDER, INK), ("بالعالم", THROUGH, RED)],
    },
    Decorated {
        family: "Noto Serif",
        size: 12,
        language: "ja",
        italic: false,
        parts: &[
            ("日本語に", PLAIN, INK),
            ("下線", UNDER, INK),
            ("と取り消し線", THROUGH, INK),
        ],
    },
];

/// Underlines and lines through text, 5 by 4 inches at 144 pixels per
/// inch, in several fonts, sizes and scripts, both directions, and
/// slanted.
pub(crate) fn decorations_page() -> Page {
    let fonts = fonts();
    let mut page = Page::new(5 * INCH, 4 * INCH);
    page.fill_background(Color::WHITE);
    let margin = INCH / 3;
    let width = page.width - 2 * margin;
    let mut y = margin;
    for para in DECORATED {
        let text: String = para.parts.iter().map(|p| p.0).collect();
        let spans: Vec<Span> = para
            .parts
            .iter()
            .map(|p| Span {
                len: p.0.len(),
                style: Style {
                    family: para.family.to_string(),
                    size: para.size * POINT,
                    weight: 400,
                    italic: para.italic,
                    language: Language::parse(para.language).expect("a language tag"),
                },
            })
            .collect();
        let layout = text::layout(
            fonts,
            &Paragraph {
                text: &text,
                spans: &spans,
                direction: None,
                align: Align::Start,
                width,
            },
        );
        let at = Point::new(margin, y);
        let colour = |span: usize| para.parts[span].2;
        page.items.extend(layout.paint(fonts, at, None, colour));
        page.items.extend(layout.decorations(
            fonts,
            0..layout.lines.len(),
            at,
            |span| para.parts[span].1,
            colour,
        ));
        y += layout.height + 5 * POINT;
    }
    page
}
