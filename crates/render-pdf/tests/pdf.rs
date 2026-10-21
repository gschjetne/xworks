//! PDF output (docs/ROADMAP.md, M1 stage 6): text extracts back as typed,
//! fonts are embedded and subset, each page has its size, and the same
//! pages give the same bytes.

use std::sync::OnceLock;

use scene::{Color, Page, Point};
use text::units::{INCH, MILLIMETRE, POINT};
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

/// One line in each M1 script, and the language it's in.
const LINES: &[(&str, &str)] = &[
    ("en", "The quick brown fox jumps over the lazy dog."),
    ("el", "Η γρήγορη καφέ αλεπού πηδάει."),
    ("ru", "Съешь же ещё этих мягких булок."),
    ("ar", "مرحبا بالعالم"),
    ("he", "שלום עולם"),
    ("ja", "日本語のテキスト"),
    ("zh-Hans", "中文排版测试"),
];

/// A page `width` by `height` with `lines` set on it, one under another.
fn page(width: i64, height: i64, lines: &[(&str, &str)], italic: bool) -> Page {
    let mut page = Page::new(width, height);
    page.fill_background(Color::WHITE);
    let mut y = INCH / 2;
    for (language, line) in lines {
        let spans = [Span {
            len: line.len(),
            style: Style {
                family: "Noto Serif".to_string(),
                size: 12 * POINT,
                weight: 400,
                italic,
                language: Language::parse(language).expect("a tag"),
            },
        }];
        let layout = text::layout(
            fonts(),
            &Paragraph {
                text: line,
                spans: &spans,
                direction: None,
                align: Align::Start,
                width: width - INCH,
            },
        );
        assert_eq!(layout.lines.len(), 1, "{line} fits on a line");
        page.items
            .extend(layout.paint(fonts(), Point::new(INCH / 2, y), |_| Color::BLACK));
        y += layout.height + 6 * POINT;
    }
    page
}

fn letter(lines: &[(&str, &str)]) -> Page {
    page(85 * INCH / 10, 11 * INCH, lines, false)
}

/// Text with its whitespace taken out, for comparing what's extracted.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The text of `items`' glyphs in the order they're drawn, left to right:
/// each glyph's text once, however many glyphs show it.
fn drawn(items: &[scene::Item]) -> String {
    let mut out = String::new();
    for item in items {
        let scene::Item::Glyphs(run) = item else {
            continue;
        };
        let mut last = None;
        for glyph in &run.glyphs {
            if last != Some(glyph.text.clone()) {
                out.push_str(&run.text[glyph.text.clone()]);
                last = Some(glyph.text.clone());
            }
        }
    }
    out
}

/// A PDF holds glyphs in the order they're drawn, left to right, each with
/// the text it shows. Readers that apply the bidi algorithm, as Acrobat and
/// poppler do, read right-to-left text back in its order; pdf-extract,
/// which reads here, doesn't, so Arabic and Hebrew come back as drawn.
/// Left-to-right text comes back exactly as typed.
#[test]
fn text_extracts_back_as_drawn() {
    let page = letter(LINES);
    let pdf = render_pdf::pdf(std::slice::from_ref(&page), Some("Scripts")).expect("a PDF");
    let extracted = squeeze(&pdf_extract::extract_text_from_mem(&pdf).expect("text extracts"));
    let mut wrong = Vec::new();
    for (language, line) in LINES {
        let single = self::page(85 * INCH / 10, 11 * INCH, &[(language, line)], false);
        let order = squeeze(&drawn(&single.items));
        let rtl = matches!(*language, "ar" | "he");
        if !rtl && order != squeeze(line) {
            wrong.push(format!("{language}: drawn as {order}, typed {line}"));
        }
        if !extracted.contains(&order) {
            wrong.push(format!("{language}: {order} isn't in what extracts"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}\nextracted: {extracted}");
}

#[test]
fn fonts_are_embedded_and_subset() {
    let pdf = render_pdf::pdf(&[letter(LINES)], None).expect("a PDF");
    let document = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    let bundled = testkit::fonts::load().expect("the bundled fonts");
    let mut embedded = Vec::new();
    for object in document.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        if dict.get(b"Type").and_then(|t| t.as_name()).ok() != Some(b"FontDescriptor".as_slice()) {
            continue;
        }
        let name = String::from_utf8_lossy(
            dict.get(b"FontName")
                .and_then(|n| n.as_name())
                .expect("a font name"),
        )
        .into_owned();
        // A subset's name is its font's, after six capitals and a plus sign.
        let (tag, base) = name
            .split_once('+')
            .unwrap_or_else(|| panic!("{name} isn't a subset"));
        assert!(
            tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()),
            "{name}"
        );
        let file = [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
            .iter()
            .find_map(|key| dict.get(key).ok())
            .unwrap_or_else(|| panic!("{name} isn't embedded"));
        let stream = document
            .get_object(file.as_reference().expect("a reference"))
            .and_then(|o| o.as_stream())
            .expect("a font stream");
        let bytes = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let original = bundled
            .iter()
            .find(|(file, _)| file.split('.').next() == Some(base))
            .unwrap_or_else(|| panic!("{base} is a bundled font"))
            .1
            .len();
        assert!(
            bytes.len() * 4 < original,
            "{name} embeds {} of its {original} bytes, more than a subset would",
            bytes.len()
        );
        embedded.push(base.to_string());
    }
    embedded.sort();
    // Japanese and Chinese share the serif CJK face.
    assert_eq!(
        embedded,
        [
            "NotoNaskhArabic-Regular",
            "NotoSerif-Regular",
            "NotoSerifCJKsc-Regular",
            "NotoSerifHebrew-Regular"
        ]
    );
}

#[test]
fn pages_have_their_sizes() {
    let a4 = page(210 * MILLIMETRE, 297 * MILLIMETRE, &LINES[..1], false);
    let pdf = render_pdf::pdf(&[letter(&LINES[..1]), a4], None).expect("a PDF");
    let document = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    let sizes: Vec<(f32, f32)> = document
        .get_pages()
        .values()
        .map(|&id| {
            let page = document.get_dictionary(id).expect("a page");
            let media = page
                .get(b"MediaBox")
                .and_then(|m| m.as_array())
                .expect("a media box");
            let n = |i: usize| media[i].as_float().expect("a number");
            (n(2) - n(0), n(3) - n(1))
        })
        .collect();
    let want = [(612.0, 792.0), (595.2756, 841.8898)];
    assert_eq!(sizes.len(), 2);
    for ((w, h), (ww, wh)) in sizes.iter().zip(want) {
        assert!((w - ww).abs() < 0.01 && (h - wh).abs() < 0.01, "{sizes:?}");
    }
}

#[test]
fn the_same_pages_give_the_same_bytes() {
    let pages = [letter(LINES), page(4 * INCH, 4 * INCH, &LINES[4..5], true)];
    assert_eq!(
        render_pdf::pdf(&pages, Some("Same")).expect("a PDF"),
        render_pdf::pdf(&pages, Some("Same")).expect("a PDF")
    );
}

#[test]
fn a_page_pdf_cant_hold_is_refused() {
    let pdf = render_pdf::pdf(&[Page::new(INCH, INCH)], None).expect("a PDF");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(matches!(
        render_pdf::pdf(&[Page::new(0, INCH)], None),
        Err(render_pdf::Error::PageSize { page: 0, .. })
    ));
}
