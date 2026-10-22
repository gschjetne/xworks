//! A story's paragraphs as styled text: styles from the sheet, with
//! character properties over them, laid out with text.

use std::sync::OnceLock;

use doc::Value;
use story::{StyleSheet, keys, props};
use text::units::POINT;
use text::{Fonts, Language, Style};

fn sheet() -> StyleSheet {
    let source = testkit::specs("styles")
        .expect("spec/styles")
        .into_iter()
        .find(|(name, _)| name == "m1")
        .expect("spec/styles/m1.ron")
        .1;
    ron::from_str(&source).expect("m1.ron is a style sheet")
}

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

fn style(family: &str, tenths_of_points: i64, weight: u16, italic: bool, language: &str) -> Style {
    Style {
        family: family.to_string(),
        size: tenths_of_points * POINT / 10,
        weight,
        italic,
        language: Language::parse(language).expect("a tag"),
    }
}

#[test]
fn runs_take_their_properties_over_their_style() {
    let document = story::document(&[
        ("Heading 1", vec![("Ölbrücke", props(&[]))]),
        (
            "Body",
            vec![
                ("plain, ", props(&[])),
                ("bold, ", props(&[(keys::BOLD, Value::Bool(true))])),
                ("italic", props(&[(keys::ITALIC, Value::Bool(true))])),
                (" and ", props(&[])),
                (
                    "日本語",
                    props(&[
                        (keys::LANGUAGE, Value::Text("ja".to_string())),
                        (keys::FONT, Value::Text("Noto Sans".to_string())),
                        (keys::SIZE, Value::Int(14 * POINT)),
                    ]),
                ),
            ],
        ),
    ]);
    let sheet = sheet();
    let paragraphs = document.paragraphs();
    let heading = story::spans(document.paragraph(paragraphs[0]).unwrap(), &sheet);
    assert_eq!(heading.len(), 1);
    assert_eq!(heading[0].len, "Ölbrücke".len());
    assert_eq!(heading[0].style, style("Inter", 200, 700, false, "und"));

    let body = story::spans(document.paragraph(paragraphs[1]).unwrap(), &sheet);
    let got: Vec<(usize, Style)> = body.into_iter().map(|s| (s.len, s.style)).collect();
    assert_eq!(
        got,
        [
            (7, style("Source Serif 4", 110, 400, false, "und")),
            (6, style("Source Serif 4", 110, 700, false, "und")),
            (6, style("Source Serif 4", 110, 400, true, "und")),
            (5, style("Source Serif 4", 110, 400, false, "und")),
            ("日本語".len(), style("Noto Sans", 140, 400, false, "ja")),
        ]
    );
}

#[test]
fn unknown_styles_and_odd_values_fall_back() {
    let mut document = story::document(&[(
        "No Such Style",
        vec![(
            "text",
            props(&[(keys::BOLD, Value::Text("yes".to_string()))]),
        )],
    )]);
    let sheet = sheet();
    let id = document.paragraphs()[0];
    let spans = story::spans(document.paragraph(id).unwrap(), &sheet);
    assert_eq!(
        spans[0].style,
        style("Source Serif 4", 110, 400, false, "und")
    );
    // A style name of the wrong kind is no style name.
    document
        .apply(&doc::Op::SetParaProp {
            paragraph: id,
            key: keys::STYLE.to_string(),
            value: Some(Value::Int(1)),
        })
        .unwrap();
    let spans = story::spans(document.paragraph(id).unwrap(), &sheet);
    assert_eq!(spans[0].style.family, "Source Serif 4");
}

#[test]
fn an_empty_paragraph_has_its_style() {
    let document = story::document(&[("Heading 2", Vec::<(&str, doc::Props)>::new())]);
    let sheet = sheet();
    let spans = story::spans(
        document.paragraph(document.paragraphs()[0]).unwrap(),
        &sheet,
    );
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].len, 0);
    assert_eq!(spans[0].style, style("Inter", 150, 700, false, "und"));
}

#[test]
fn paragraphs_lay_out_with_their_spacing() {
    let document = story::document(&[
        ("Heading 1", vec![("A heading", props(&[]))]),
        (
            "Body",
            vec![(
                "Some text that runs on long enough to need more than one line at this width.",
                props(&[]),
            )],
        ),
    ]);
    let laid = story::lay_out(&document, &sheet(), fonts(), 200 * POINT);
    assert_eq!(laid.len(), 2);
    assert_eq!(laid[0].id, document.paragraphs()[0]);
    assert_eq!(
        (
            laid[0].space_before,
            laid[0].space_after,
            laid[0].keep_with_next
        ),
        (18 * POINT, 6 * POINT, true)
    );
    assert_eq!(
        (
            laid[1].space_before,
            laid[1].space_after,
            laid[1].keep_with_next
        ),
        (0, 6 * POINT, false)
    );
    assert_eq!((laid[0].heading, laid[1].heading), (Some(1), None));
    assert!(laid[1].layout.lines.len() > 1);
}

/// Every hyphenation pattern, as the app loads them.
fn hyphenation() -> &'static text::Hyphenation {
    static HYPHENATION: OnceLock<text::Hyphenation> = OnceLock::new();
    HYPHENATION.get_or_init(|| {
        let mut all = text::Hyphenation::new();
        for loaded in testkit::hyphenation::load().expect("the patterns") {
            let l = &loaded.language;
            let h = std::sync::Arc::new(text::Hyphenator::parse(
                &loaded.patterns,
                &loaded.exceptions,
                l.left,
                l.right,
            ));
            for tag in &l.tags {
                all.insert(&Language::parse(tag).expect("a tag"), h.clone());
            }
        }
        all
    })
}

#[test]
fn justified_paragraphs_hyphenate_and_ragged_ones_dont() {
    let text = "Hyphenation and justification together make typesetting even.";
    let mut document = story::document(&[("Body", vec![(text, props(&[]))])]);
    let id = document.paragraphs()[0];
    document
        .apply(&doc::Op::SetParaProp {
            paragraph: id,
            key: keys::LANGUAGE.to_string(),
            value: Some(Value::Text("en-US".to_string())),
        })
        .unwrap();
    let sheet = sheet();
    let lay_out = |d: &doc::Document| {
        story::lay_out_paragraph_with(d, id, &sheet, fonts(), hyphenation(), 90 * POINT)
    };
    assert!(lay_out(&document).layout.lines.iter().all(|l| !l.hyphen));
    document
        .apply(&doc::Op::SetParaProp {
            paragraph: id,
            key: keys::ALIGN.to_string(),
            value: Some(Value::Text("justify".to_string())),
        })
        .unwrap();
    assert_eq!(
        story::align(document.paragraph(id).unwrap(), &sheet),
        text::Align::Justify
    );
    let justified = lay_out(&document);
    assert!(justified.layout.lines.iter().any(|l| l.hyphen));
    assert!(justified.layout.unhyphenated.is_empty());
    // Without the patterns, the layout asks for them.
    let without = story::lay_out_paragraph(&document, id, &sheet, fonts(), 90 * POINT);
    assert_eq!(
        without.layout.unhyphenated,
        [Language::parse("en-US").unwrap()]
    );
}

#[test]
fn tab_stops_are_the_styles_and_the_documents() {
    let mut sheet = sheet();
    let stop = |pt: f64, align| story::TabStopSpec {
        at: text::units::Length::Pt(pt),
        align,
    };
    sheet.tabs.stops = vec![
        stop(100.0, text::TabAlign::Start),
        stop(50.0, text::TabAlign::Start),
    ];
    let mut style = sheet.paragraph[0].clone();
    style.tabs = vec![
        stop(100.0, text::TabAlign::End),
        stop(200.0, text::TabAlign::Center),
    ];
    let tabs = sheet.tabs(&style);
    let got: Vec<(i64, text::TabAlign)> =
        tabs.stops.iter().map(|s| (s.position, s.align)).collect();
    assert_eq!(
        got,
        [
            (50 * POINT, text::TabAlign::Start),
            (100 * POINT, text::TabAlign::End),
            (200 * POINT, text::TabAlign::Center),
        ]
    );
    // The document's default stops, from spec/styles/.
    assert!(tabs.interval > 0);
}

#[test]
fn words_are_counted_by_their_language() {
    let document = story::document(&[(
        "Body",
        vec![
            ("Two words, ", props(&[])),
            (
                "\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\u{4e16}\u{754c}",
                props(&[(keys::LANGUAGE, Value::Text("ja".to_string()))]),
            ),
        ],
    )]);
    let paragraph = document.paragraph(document.paragraphs()[0]).unwrap();
    // "Two", "words", and, from ICU4X's Japanese dictionary, the two words
    // of "hello, world".
    assert_eq!(story::word_count(paragraph, &sheet()), 4);
}
