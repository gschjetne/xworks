//! Drawing only what changed (docs/ROADMAP.md, M2 stage 4): an item's
//! bounds hold everything it draws, drawing a new display list's damage
//! over the old one's pixels gives the new one's, and a renderer draws a
//! page the same whatever it drew before.

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
            ligatures: text::Ligatures::default(),
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

/// A renderer keeps what it makes of images from one render to the next,
/// but what it draws doesn't depend on that: after drawing other pages,
/// other images and parts of pages, it draws a page exactly as a fresh
/// one does, every time.
#[test]
fn a_renderer_draws_the_same_whatever_it_drew_before() {
    let ppi = 96.0;
    let picture = |seed: u8| {
        scene::Image::new(
            8,
            8,
            (0..64u8)
                .flat_map(|i| [i.wrapping_mul(seed), 255 - i * 4, seed, 255])
                .collect(),
        )
        .expect("an image")
    };
    let page = |seed: u8, x: i64| {
        window(
            [
                lines(),
                vec![Item::Image {
                    image: picture(seed),
                    rect: Rect::new(x, 2 * INCH, INCH / 2, INCH / 3),
                    alt: None,
                    source: None,
                }],
            ]
            .into_iter()
            .flatten()
            .collect(),
        )
    };
    let (first, second) = (page(3, INCH), page(7, 2 * INCH));
    let fresh = render_cpu::render(&first, ppi, Level::Portable);
    let mut renderer = Renderer::new();
    assert_eq!(renderer.render(&first, ppi, Level::Portable), fresh);
    renderer.render(&second, ppi, Level::Portable);
    renderer.render_rect(
        &second,
        ppi,
        Level::Portable,
        PixelRect {
            x: 90,
            y: 180,
            width: 100,
            height: 60,
        },
    );
    renderer.render(&first, 2.0 * ppi, Level::Portable);
    assert_eq!(renderer.render(&first, ppi, Level::Portable), fresh);
    assert_eq!(render_cpu::render(&first, ppi, Level::Portable), fresh);
}

/// `run` drawn on a page with a fill after it in its top left inch:
/// whether the fill shows.
fn drawn_after(run: scene::Glyphs) -> bool {
    let mut page = Page::new(6 * INCH, 3 * INCH);
    page.items.push(Item::Glyphs(run));
    page.items.push(Item::Fill {
        path: Path::rect(0, 0, INCH, INCH),
        color: Color::rgb(10, 20, 30),
    });
    let image = render_cpu::render(&page, 96.0, Level::Portable);
    pixel(&image, 10, 10) == [10, 20, 30, 255]
}

/// The first glyph run of [`lines`].
fn first_run() -> scene::Glyphs {
    lines()
        .into_iter()
        .find_map(|item| match item {
            Item::Glyphs(run) => Some(run),
            _ => None,
        })
        .expect("a glyph run")
}

/// Glyphs a font hasn't don't stop the page: there's no panic, and the
/// rest of it draws.
#[test]
fn glyphs_a_font_hasnt_leave_the_rest_of_the_page() {
    let mut run = first_run();
    for glyph in &mut run.glyphs {
        glyph.id = u32::MAX - glyph.id;
    }
    assert!(drawn_after(run));
}

/// A run whose font isn't one, or names a face its file hasn't, should
/// leave the rest of the page too, as render-cpu's own comment says; it
/// panics in glifo 0.4.0, which unwraps the font it reads. `text` only
/// makes runs of faces it loaded, so no document reaches it.
#[test]
#[ignore = "panics in glifo: M2 stage 8 part A's report, render-cpu"]
fn a_run_whose_font_isnt_one_leaves_the_rest_of_the_page() {
    let mut junk = first_run();
    junk.font = scene::Font {
        data: std::sync::Arc::new(vec![0u8; 64]),
        index: 0,
    };
    assert!(drawn_after(junk));
    let mut far = first_run();
    far.font.index = 7;
    assert!(drawn_after(far));
}
