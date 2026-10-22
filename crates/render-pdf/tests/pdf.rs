//! PDF output (docs/ROADMAP.md, M1 stage 6): a tagged PDF whose text
//! extracts back as typed, in every script; fonts embedded and subset;
//! each page its size; and the same pages giving the same bytes.

use std::sync::OnceLock;

use scene::{Color, Page, Point, Role, Structure};
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

/// A page `width` by `height` with `lines` set on it, one under another,
/// each a block of the structure from `first` on.
fn page_from(width: i64, height: i64, lines: &[(&str, &str)], italic: bool, first: u32) -> Page {
    let mut page = Page::new(width, height);
    page.fill_background(Color::WHITE);
    let mut y = INCH / 2;
    for (i, (language, line)) in lines.iter().enumerate() {
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
        page.items.extend(layout.paint(
            fonts(),
            Point::new(INCH / 2, y),
            Some(first + i as u32),
            |_| Color::BLACK,
        ));
        y += layout.height + 6 * POINT;
    }
    page
}

fn page(width: i64, height: i64, lines: &[(&str, &str)], italic: bool) -> Page {
    page_from(width, height, lines, italic, 0)
}

fn letter(lines: &[(&str, &str)]) -> Page {
    page(85 * INCH / 10, 11 * INCH, lines, false)
}

/// A structure of `n` paragraphs.
fn paragraphs(n: usize) -> Structure {
    Structure {
        language: Some("en".to_string()),
        blocks: vec![Role::Paragraph; n],
        parents: Vec::new(),
    }
}

/// Text with its whitespace taken out, for comparing what's extracted.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Lines whose order on screen isn't their order in the text: a number in
/// Arabic, and a Hebrew word in English.
const MIXED: &[(&str, &str)] = &[
    ("ar", "عام 2026 هو العام"),
    ("en", "The word שלום means peace."),
];

#[test]
fn text_extracts_back_as_typed() {
    let lines: Vec<(&str, &str)> = LINES.iter().chain(MIXED).copied().collect();
    let pdf = render_pdf::pdf(&[letter(&lines)], &paragraphs(lines.len()), Some("Scripts"))
        .expect("a PDF");
    let extracted = squeeze(&pdf_extract::extract_text_from_mem(&pdf).expect("text extracts"));
    let missing: Vec<_> = lines
        .iter()
        .filter(|(_, line)| !extracted.contains(&squeeze(line)))
        .collect();
    assert!(
        missing.is_empty(),
        "not extracted: {missing:?}\nfrom: {extracted}"
    );
    // And by the second reader, which honours krilla's ActualText where a
    // glyph shows different letters in different places, as Arabic's
    // dotless forms do.
    let read = squeeze(&testkit::pdf::text(&pdf).expect("text reads"));
    let missing: Vec<_> = lines
        .iter()
        .filter(|(_, line)| !read.contains(&squeeze(line)))
        .collect();
    assert!(missing.is_empty(), "not read: {missing:?}\nfrom: {read}");
}

/// The structure elements of `pdf`, by type, in the tree's order, each
/// with the number of its kids.
fn elements(pdf: &[u8]) -> Vec<(String, usize)> {
    let document = lopdf::Document::load_mem(pdf).expect("the PDF reads");
    let catalog = document.catalog().expect("a catalog");
    let marked = catalog
        .get(b"MarkInfo")
        .and_then(|m| document.dereference(m))
        .and_then(|(_, m)| m.as_dict())
        .and_then(|m| m.get(b"Marked"))
        .and_then(|m| m.as_bool())
        .expect("MarkInfo says whether it's tagged");
    assert!(marked, "the PDF is marked as tagged");
    let root = catalog
        .get(b"StructTreeRoot")
        .and_then(|r| document.dereference(r))
        .and_then(|(_, r)| r.as_dict())
        .expect("a structure tree");
    let mut out = Vec::new();
    let mut stack = vec![root.get(b"K").expect("the root has kids").clone()];
    while let Some(object) = stack.pop() {
        let (_, object) = document.dereference(&object).expect("a kid");
        match object {
            lopdf::Object::Array(kids) => stack.extend(kids.iter().rev().cloned()),
            lopdf::Object::Dictionary(dict) => {
                let Ok(kind) = dict.get(b"S").and_then(|s| s.as_name()) else {
                    continue;
                };
                let kind = String::from_utf8_lossy(kind).into_owned();
                let kids = match dict.get(b"K") {
                    Ok(lopdf::Object::Array(kids)) => kids.clone(),
                    Ok(kid) => vec![kid.clone()],
                    Err(_) => Vec::new(),
                };
                out.push((kind.clone(), kids.len()));
                if kind == "Document" || kind == "Part" || kind == "Sect" {
                    stack.extend(kids.into_iter().rev());
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn the_pdf_is_tagged_with_the_documents_structure() {
    // A heading, then a paragraph that runs over two pages, then one more.
    let heading = page_from(4 * INCH, 4 * INCH, &[("en", "A heading")], false, 0);
    let mut first = page_from(
        4 * INCH,
        4 * INCH,
        &[("en", "The paragraph starts")],
        false,
        1,
    );
    first.place(&heading, Point::new(0, INCH));
    let second = page_from(
        4 * INCH,
        4 * INCH,
        &[("en", "and ends here."), ("he", "שלום עולם")],
        false,
        1,
    );
    let structure = Structure {
        language: Some("en".to_string()),
        blocks: vec![Role::Heading(1), Role::Paragraph, Role::Paragraph],
        parents: Vec::new(),
    };
    let pdf = render_pdf::pdf(&[first, second], &structure, Some("Tagged")).expect("a PDF");
    let found: Vec<(String, usize)> = elements(&pdf)
        .into_iter()
        .filter(|(kind, _)| matches!(kind.as_str(), "H1" | "P"))
        .collect();
    assert_eq!(
        found,
        [
            ("H1".to_string(), 1),
            ("P".to_string(), 2),
            ("P".to_string(), 1)
        ]
    );
    // Spans say their language.
    let document = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    let content = document
        .get_page_content(*document.get_pages().get(&2).expect("page 2"))
        .expect("page 2's content");
    let content = String::from_utf8_lossy(&content);
    assert!(content.contains("/Lang(he)"), "{content}");
}

#[test]
fn fonts_are_embedded_and_subset() {
    let pdf = render_pdf::pdf(&[letter(LINES)], &paragraphs(LINES.len()), None).expect("a PDF");
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
    let pdf = render_pdf::pdf(&[letter(&LINES[..1]), a4], &paragraphs(1), None).expect("a PDF");
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
    let structure = paragraphs(LINES.len());
    assert_eq!(
        render_pdf::pdf(&pages, &structure, Some("Same")).expect("a PDF"),
        render_pdf::pdf(&pages, &structure, Some("Same")).expect("a PDF")
    );
}

/// Every structure element of `pdf` under the document, in the tree's
/// order, as its depth, its type and how many kids it has.
fn tree(pdf: &[u8]) -> Vec<(usize, String, usize)> {
    let document = lopdf::Document::load_mem(pdf).expect("the PDF reads");
    let root = document
        .catalog()
        .expect("a catalog")
        .get(b"StructTreeRoot")
        .and_then(|r| document.dereference(r))
        .and_then(|(_, r)| r.as_dict())
        .expect("a structure tree");
    let mut out = Vec::new();
    let mut stack = vec![(0, root.get(b"K").expect("the root has kids").clone())];
    while let Some((depth, object)) = stack.pop() {
        let (_, object) = document.dereference(&object).expect("a kid");
        match object {
            lopdf::Object::Array(kids) => {
                stack.extend(kids.iter().rev().cloned().map(|k| (depth, k)));
            }
            lopdf::Object::Dictionary(dict) => {
                let Ok(kind) = dict.get(b"S").and_then(|s| s.as_name()) else {
                    continue;
                };
                let kind = String::from_utf8_lossy(kind).into_owned();
                let kids = match dict.get(b"K") {
                    Ok(lopdf::Object::Array(kids)) => kids.clone(),
                    Ok(kid) => vec![kid.clone()],
                    Err(_) => Vec::new(),
                };
                let below = if kind == "Document" { 0 } else { depth + 1 };
                if kind != "Document" {
                    out.push((depth, kind, kids.len()));
                }
                stack.extend(kids.into_iter().rev().map(|k| (below, k)));
            }
            _ => {}
        }
    }
    out
}

/// Lists, quotes, code and tables nest as tagged PDF nests them: L, LI,
/// Lbl and LBody; BlockQuote; Code; Table, TR, TH and TD.
#[test]
fn blocks_nest_in_the_tags() {
    use Role::*;
    let roles = [
        (List { ordered: false }, None),
        (ListItem, Some(0)),
        (Label, Some(1)),
        (ListBody, Some(1)),
        (Paragraph, Some(3)),
        (Quote, None),
        (Paragraph, Some(5)),
        (Table, None),
        (Row, Some(7)),
        (Cell { header: true }, Some(8)),
        (Row, Some(7)),
        (Cell { header: false }, Some(10)),
        (Code, None),
        // A list with nothing drawn in it isn't tagged.
        (List { ordered: true }, None),
        (ListItem, Some(13)),
    ];
    let structure = Structure {
        language: Some("en".to_string()),
        blocks: roles.iter().map(|r| r.0).collect(),
        parents: roles.iter().map(|r| r.1).collect(),
    };
    let mut page = Page::new(4 * INCH, 4 * INCH);
    page.fill_background(Color::WHITE);
    for (n, (block, text)) in [
        (2, "•"),
        (4, "one"),
        (6, "quoted"),
        (9, "Name"),
        (11, "a"),
        (12, "let x = 1;"),
    ]
    .into_iter()
    .enumerate()
    {
        let one = page_from(4 * INCH, INCH, &[("en", text)], false, block);
        page.place(&one, Point::new(0, n as i64 * INCH / 2));
    }
    let pdf = render_pdf::pdf(&[page], &structure, Some("Blocks")).expect("a PDF");
    let found: Vec<(usize, String, usize)> = tree(&pdf)
        .into_iter()
        .filter(|(_, kind, _)| kind != "Span")
        .collect();
    let want: Vec<(usize, String, usize)> = [
        (0, "L", 1),
        (1, "LI", 2),
        (2, "Lbl", 1),
        (2, "LBody", 1),
        (3, "P", 1),
        (0, "BlockQuote", 1),
        (1, "P", 1),
        (0, "Table", 2),
        (1, "TR", 1),
        (2, "TH", 1),
        (1, "TR", 1),
        (2, "TD", 1),
        (0, "Code", 1),
    ]
    .into_iter()
    .map(|(d, k, n)| (d, k.to_string(), n))
    .collect();
    assert_eq!(found, want);
    let text = squeeze(&pdf_extract::extract_text_from_mem(&pdf).expect("text extracts"));
    assert!(text.contains("•onequotedNamealetx=1;"), "{text}");
}

#[test]
fn a_page_pdf_cant_hold_is_refused() {
    let none = Structure::default();
    let pdf = render_pdf::pdf(&[Page::new(INCH, INCH)], &none, None).expect("a PDF");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(matches!(
        render_pdf::pdf(&[Page::new(0, INCH)], &none, None),
        Err(render_pdf::Error::PageSize { page: 0, .. })
    ));
}

/// A 3 by 2 image: a red row over a half-transparent blue one.
fn picture() -> scene::Image {
    let rgba = [[255, 0, 0, 255]; 3]
        .into_iter()
        .chain([[0, 0, 255, 128]; 3])
        .flatten()
        .collect();
    scene::Image::new(3, 2, rgba).expect("an image")
}

#[test]
fn images_are_figures_with_their_alternative_text() {
    let mut page = page_from(
        4 * INCH,
        4 * INCH,
        &[("en", "Before the figure.")],
        false,
        0,
    );
    let image = picture();
    page.items.push(scene::Item::Image {
        image: image.clone(),
        rect: scene::Rect::new(INCH, INCH, 3 * INCH / 2, INCH),
        alt: Some("A red stripe over a blue one".to_string()),
        source: Some(scene::Source {
            block: 1,
            offset: 0,
        }),
    });
    // An image of no block is a figure of its own, after the structure.
    page.items.push(scene::Item::Image {
        image,
        rect: scene::Rect::new(INCH, 3 * INCH, INCH, INCH),
        alt: None,
        source: None,
    });
    let structure = Structure {
        language: Some("en".to_string()),
        blocks: vec![Role::Paragraph, Role::Figure],
        parents: Vec::new(),
    };
    let pdf = render_pdf::pdf(&[page], &structure, Some("Figures")).expect("a PDF");
    let found: Vec<(String, usize)> = elements(&pdf)
        .into_iter()
        .filter(|(kind, _)| matches!(kind.as_str(), "P" | "Figure"))
        .collect();
    assert_eq!(
        found,
        [
            ("P".to_string(), 1),
            ("Figure".to_string(), 1),
            ("Figure".to_string(), 1)
        ]
    );
    let document = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    let mut alts = Vec::new();
    let mut images = Vec::new();
    for object in document.objects.values() {
        if let Ok(dict) = object.as_dict()
            && let Ok(alt) = dict.get(b"Alt")
        {
            alts.push(lopdf::decode_text_string(alt).expect("text"));
        }
        if let Ok(stream) = object.as_stream()
            && stream.dict.get(b"Subtype").and_then(|s| s.as_name()).ok() == Some(b"Image")
        {
            let n = |key: &[u8]| stream.dict.get(key).and_then(|v| v.as_i64()).unwrap_or(0);
            images.push((n(b"Width"), n(b"Height"), stream.dict.has(b"SMask")));
        }
    }
    assert_eq!(alts, ["A red stripe over a blue one"]);
    // The same image twice is written once, its alpha as a soft mask,
    // which is an image too.
    images.sort();
    assert_eq!(images, [(3, 2, false), (3, 2, true)]);
}

#[test]
fn groups_are_clipped_and_transformed_in_the_pdf_too() {
    let line = page_from(4 * INCH, 4 * INCH, &[("en", "Turned text")], false, 0);
    let mut page = Page::new(4 * INCH, 4 * INCH);
    page.items.push(scene::Item::Group(scene::Group {
        clip: Some(scene::Path::rect(0, 0, 2 * INCH, 4 * INCH)),
        transform: scene::Transform::rotate(90.0).then(scene::Transform::translate(3 * INCH, 0)),
        items: line.items,
    }));
    let pdf = render_pdf::pdf(&[page], &paragraphs(1), None).expect("a PDF");
    let extracted = squeeze(&pdf_extract::extract_text_from_mem(&pdf).expect("text extracts"));
    assert!(extracted.contains("Turnedtext"), "{extracted}");
    let document = lopdf::Document::load_mem(&pdf).expect("the PDF reads");
    let content = document
        .get_page_content(*document.get_pages().get(&1).expect("page 1"))
        .expect("page 1's content");
    let content = String::from_utf8_lossy(&content);
    // A clip, then the quarter turn, which krilla writes with PDF's page,
    // whose y grows up, turned the other way.
    assert!(content.contains("W\nn"), "{content}");
    assert!(content.contains("0 -1 -1 0 216 288 cm"), "{content}");
}
