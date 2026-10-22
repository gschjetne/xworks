//! Drawing only what changed (docs/ROADMAP.md, M2 stage 4): an item's
//! bounds hold everything it draws, and drawing a new display list's
//! damage over the old one's pixels gives the new one's.

use std::sync::OnceLock;

use render_cpu::{Image, Level, Renderer};
use scene::{Color, Group, Item, Page, Path, PixelRect, Point, Rect, Transform};
use text::units::{INCH, PIXEL, POINT};
use text::{Align, Fonts, Language, Paragraph, Span, Style};

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Fonts::new();
        for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
            fonts.add(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let spec: text::FontSpec =
            ron::from_str(&testkit::fonts::spec().expect("fonts.ron")).expect("valid");
        fonts.set_fallbacks(&spec);
        fonts
    })
}

/// `text` set at `at`, `width` wide.
fn paragraph(text: &str, family: &str, italic: bool, at: Point, width: i64) -> Vec<Item> {
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: family.to_string(),
            size: 14 * POINT,
            weight: 400,
            italic,
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

/// Lines in many scripts and slants, whose glyphs reach above and below
/// the usual.
fn lines() -> Vec<Item> {
    let mut items = Vec::new();
    let texts = [
        ("Ágþ Ŷ̃ fjord ÅÄÖ — {[()]} ṩ̂", "Noto Serif", false),
        ("Slanted: שָׁלוֹם and مَرْحَبًا", "Noto Serif", true),
        ("Ελληνικά, Кириллица, 日本語の縦書き", "Noto Sans", false),
        ("بِسْمِ ٱللَّٰهِ ٱلرَّحْمَٰنِ", "Noto Sans", false),
    ];
    for (i, (text, family, italic)) in texts.into_iter().enumerate() {
        let at = Point::new(INCH / 4, INCH / 4 + i as i64 * INCH / 2);
        items.extend(paragraph(text, family, italic, at, 5 * INCH));
    }
    items
}

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    image.pixels[i..i + 4].try_into().unwrap()
}

#[test]
fn every_item_draws_within_its_bounds() {
    let ppi = 192.0;
    let (width, height) = (6 * INCH, 3 * INCH);
    let mut items = lines();
    items.push(Item::Stroke {
        path: Path(vec![
            scene::Segment::MoveTo(Point::new(INCH, 2 * INCH)),
            scene::Segment::LineTo(Point::new(2 * INCH, 5 * INCH / 2)),
            scene::Segment::LineTo(Point::new(3 * INCH, 2 * INCH)),
        ]),
        width: 6 * POINT,
        color: Color::BLACK,
    });
    items.push(Item::Group(Group {
        clip: None,
        transform: Transform([0.8, 0.6, -0.6, 0.8, (4 * INCH) as f64, (INCH * 2) as f64]),
        items: paragraph("Turned", "Noto Serif", true, Point::new(0, 0), INCH * 2),
    }));
    for (n, item) in items.iter().enumerate() {
        let mut page = Page::new(width, height);
        page.items.push(item.clone());
        let image = render_cpu::render(&page, ppi, Level::Portable);
        let bounds = item
            .bounds()
            .and_then(|b| b.pixels(ppi, image.width, image.height))
            .expect("it's on the page");
        for y in 0..image.height {
            for x in 0..image.width {
                let inside = (bounds.x..bounds.x + bounds.width).contains(&x)
                    && (bounds.y..bounds.y + bounds.height).contains(&y);
                assert!(
                    inside || pixel(&image, x, y)[3] == 0,
                    "item {n} draws at ({x}, {y}), outside {bounds:?}"
                );
            }
        }
    }
}

/// `old`'s pixels with `new`'s damage drawn over them, as a window would.
fn redrawn(old: &Page, new: &Page, ppi: f64) -> (Image, Vec<PixelRect>) {
    let mut renderer = Renderer::new();
    let mut image = renderer.render(old, ppi, Level::Portable);
    let rects = scene::redraw(old, new, ppi, image.width, image.height);
    for rect in &rects {
        let part = renderer.render_rect(new, ppi, Level::Portable, *rect);
        for y in 0..rect.height {
            for x in 0..rect.width {
                let from = ((y * part.width + x) * 4) as usize;
                let to = (((rect.y + y) * image.width + rect.x + x) * 4) as usize;
                image.pixels[to..to + 4].copy_from_slice(&part.pixels[from..from + 4]);
            }
        }
    }
    (image, rects)
}

fn window(items: Vec<Item>) -> Page {
    let mut page = Page::new(6 * INCH, 3 * INCH);
    page.fill_background(Color::rgb(230, 232, 236));
    page.items.push(Item::Fill {
        path: Path::rect(INCH / 8, INCH / 8, 23 * INCH / 4, 11 * INCH / 4),
        color: Color::WHITE,
    });
    page.items.extend(items);
    page
}

#[test]
fn drawing_the_damage_over_the_old_frame_gives_the_new_one() {
    let ppi = 96.0;
    let caret = |x: i64| Item::Fill {
        path: Path::rect(x, INCH / 4, PIXEL * 3 / 2, 14 * POINT),
        color: Color::BLACK,
    };
    let picture = scene::Image::new(
        4,
        4,
        (0..16u8)
            .flat_map(|i| [i * 16, 255 - i * 16, 128, 255])
            .collect(),
    )
    .expect("an image");
    let image = |x: i64| Item::Image {
        image: picture.clone(),
        rect: Rect::new(x, 2 * INCH, INCH / 2, INCH / 2),
        alt: None,
        source: None,
    };
    let old = window(
        [lines(), vec![caret(INCH), image(INCH)]]
            .into_iter()
            .flatten()
            .collect(),
    );
    // A word typed in the first line, the caret moved, the image moved.
    let mut changed = lines();
    changed.splice(
        0..1,
        paragraph(
            "Ágþ Ŷ̃ fjords ÅÄÖ — {[()]} ṩ̂",
            "Noto Serif",
            false,
            Point::new(INCH / 4, INCH / 4),
            5 * INCH,
        ),
    );
    let new = window(
        [changed, vec![caret(2 * INCH), image(3 * INCH)]]
            .into_iter()
            .flatten()
            .collect(),
    );
    let (image, rects) = redrawn(&old, &new, ppi);
    let whole = render_cpu::render(&new, ppi, Level::Portable);
    let area: u64 = rects
        .iter()
        .map(|r| u64::from(r.width) * u64::from(r.height))
        .sum();
    assert!(
        area * 2 < u64::from(whole.width) * u64::from(whole.height),
        "the damage is a part of the window: {rects:?}"
    );
    let worst = image
        .pixels
        .iter()
        .zip(&whole.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(worst <= 1, "{worst} levels apart");
    // Nothing changed, nothing drawn.
    assert_eq!(scene::redraw(&new, &new, ppi, 576, 288), []);
}
