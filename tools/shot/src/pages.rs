//! The pages behind the scenes, as display lists, so PDF output can draw
//! the same ones.

use std::sync::OnceLock;

use scene::{Color, Item, Page, Path, Point, Segment};
use text::units::{INCH, POINT};
use text::{Align, Fonts, Language, Paragraph, Span, Style};

/// The bundled fonts, loaded once.
pub fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Fonts::new();
        let files = testkit::fonts::load()
            .expect("the bundled fonts; fetch them with `cargo run -p fetch-fonts`");
        for (name, bytes) in files {
            fonts
                .add(bytes)
                .unwrap_or_else(|e| panic!("loading {name}: {e}"));
        }
        let spec: text::FontSpec =
            ron::from_str(&testkit::fonts::spec().expect("spec/text/fonts.ron"))
                .expect("spec/text/fonts.ron is valid");
        fonts.set_fallbacks(&spec);
        fonts
    })
}

/// A paragraph's stretch: its text and how it differs from the
/// paragraph's style.
struct Part {
    text: &'static str,
    bold: bool,
    italic: bool,
    color: Color,
}

const fn plain(text: &'static str) -> Part {
    Part {
        text,
        bold: false,
        italic: false,
        color: Color::BLACK,
    }
}

/// A paragraph of the text page.
struct Para {
    family: &'static str,
    size: i64,
    language: &'static str,
    parts: &'static [Part],
}

const ACCENT: Color = Color::rgb(160, 30, 40);

const TEXT: &[Para] = &[
    Para {
        family: "Noto Sans",
        size: 18,
        language: "en",
        parts: &[Part {
            text: "xWorks sets text",
            bold: true,
            italic: false,
            color: Color::BLACK,
        }],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "en",
        parts: &[
            plain("Text comes in "),
            Part {
                text: "bold",
                bold: true,
                italic: false,
                color: Color::BLACK,
            },
            plain(", "),
            Part {
                text: "italic",
                bold: false,
                italic: true,
                color: Color::BLACK,
            },
            plain(" and "),
            Part {
                text: "colour",
                bold: false,
                italic: false,
                color: ACCENT,
            },
            plain(
                ", with fine ligatures and kerning: AVATAR, Wolf, office. The quick brown fox jumps over the lazy dog.",
            ),
        ],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "el",
        parts: &[plain(
            "Η γρήγορη καφέ αλεπού πηδάει πάνω από τον τεμπέλη σκύλο.",
        )],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "ru",
        parts: &[plain(
            "Съешь же ещё этих мягких французских булок, да выпей чаю.",
        )],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "ar",
        parts: &[plain(
            "مرحبا بالعالم، هذا نص عربي للاختبار. عام 2026 هو العام.",
        )],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "he",
        parts: &[
            plain("שלום עולם, זהו טקסט "),
            Part {
                text: "נטוי",
                bold: false,
                italic: true,
                color: Color::BLACK,
            },
            plain(" לבדיקה."),
        ],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "ja",
        parts: &[plain("日本語のテキストは単語の間で改行されます。")],
    },
    Para {
        family: "Noto Sans",
        size: 11,
        language: "zh-Hans",
        parts: &[plain("中文排版：标点符号不会出现在行首。")],
    },
    Para {
        family: "Noto Serif",
        size: 11,
        language: "en",
        parts: &[plain("The word שלום means peace, and مرحبا means hello.")],
    },
];

/// A page of paragraphs in every M1 script, 5 by 6 inches.
pub fn text_page() -> Page {
    let fonts = fonts();
    let mut page = Page::new(5 * INCH, 6 * INCH);
    page.fill_background(Color::WHITE);
    let margin = INCH / 2;
    let width = page.width - 2 * margin;
    let mut y = margin;
    for para in TEXT {
        let text: String = para.parts.iter().map(|p| p.text).collect();
        let spans: Vec<Span> = para
            .parts
            .iter()
            .map(|p| Span {
                len: p.text.len(),
                style: Style {
                    family: para.family.to_string(),
                    size: para.size * POINT,
                    weight: if p.bold { 700 } else { 400 },
                    italic: p.italic,
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
        page.items
            .extend(layout.paint(fonts, Point::new(margin, y), |span| para.parts[span].color));
        y += layout.height + 6 * POINT;
    }
    page
}

/// A page of shapes, 4 by 3 inches: fills, strokes, curves, and
/// transparent colours over each other.
pub fn shapes_page() -> Page {
    let u = INCH / 8;
    let p = |x: i64, y: i64| Point::new(x * u, y * u);
    let mut page = Page::new(32 * u, 24 * u);
    page.fill_background(Color::WHITE);
    page.items.push(Item::Fill {
        path: Path::rect(2 * u, 2 * u, 10 * u, 6 * u),
        color: Color::rgb(32, 48, 96),
    });
    // A circle from four cubic arcs.
    let k = 552; // 0.552 of the radius: the usual cubic circle.
    let (cx, cy, r) = (20 * u, 6 * u, 4 * u);
    let c = |dx: i64, dy: i64| Point::new(cx + dx, cy + dy);
    let kr = r * k / 1000;
    page.items.push(Item::Fill {
        path: Path(vec![
            Segment::MoveTo(c(r, 0)),
            Segment::CubicTo(c(r, kr), c(kr, r), c(0, r)),
            Segment::CubicTo(c(-kr, r), c(-r, kr), c(-r, 0)),
            Segment::CubicTo(c(-r, -kr), c(-kr, -r), c(0, -r)),
            Segment::CubicTo(c(kr, -r), c(r, -kr), c(r, 0)),
            Segment::Close,
        ]),
        color: Color::rgb(220, 120, 40),
    });
    page.items.push(Item::Fill {
        path: Path::rect(16 * u, 4 * u, 12 * u, 6 * u),
        color: Color {
            r: 40,
            g: 120,
            b: 220,
            a: 128,
        },
    });
    page.items.push(Item::Stroke {
        path: Path(vec![
            Segment::MoveTo(p(2, 20)),
            Segment::LineTo(p(8, 12)),
            Segment::QuadTo(p(14, 4), p(18, 18)),
            Segment::LineTo(p(30, 14)),
        ]),
        width: 4 * POINT,
        color: Color::rgb(30, 130, 60),
    });
    page.items.push(Item::Stroke {
        path: Path::rect(u, u, 30 * u, 22 * u),
        width: POINT,
        color: Color::BLACK,
    });
    page
}
