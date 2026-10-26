//! A document's own stylesheet (M2 stage 9; docs/ARCHITECTURE.md, section
//! 6): stylesheets as files, made a document's own; styles resolved along
//! chains of any depth, named parts taken for their values, and where each
//! value comes from; marks as character styles; and one document's styles
//! brought into another's, matched by role and name.

use doc::{Document, ObjectId, Op, Props, Value};
use story::styles::{Level, find, valid};
use story::{SheetError, SheetSpec, StyleSheet, keys, props};
use text::units::{MILLIMETRE, POINT};

/// Every stylesheet that comes with xWorks, by name, as `spec/styles.ron`
/// lists them.
fn stylesheets() -> Vec<(String, SheetSpec)> {
    #[derive(serde::Deserialize)]
    struct List {
        stylesheets: Vec<String>,
    }
    let list = testkit::specs("")
        .expect("spec/")
        .into_iter()
        .find(|(n, _)| n == "styles")
        .expect("spec/styles.ron")
        .1;
    let list: List = ron::from_str(&list).expect("a list");
    let files = testkit::specs("styles").expect("spec/styles/");
    list.stylesheets
        .into_iter()
        .map(|name| {
            let text = &files
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("spec/styles/{name}.ron"))
                .1;
            let spec = SheetSpec::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, spec)
        })
        .collect()
}

fn neutral() -> SheetSpec {
    stylesheets()
        .into_iter()
        .find(|(n, _)| n == "neutral")
        .expect("Neutral")
        .1
}

/// The ID of `document`'s style or part of `kind` named `name`.
fn named(document: &Document, kind: doc::StyleKind, name: &str) -> ObjectId {
    document
        .styles()
        .iter()
        .copied()
        .find(|&s| {
            let style = document.style(s).unwrap();
            style.kind == kind && style.props.get(keys::NAME) == Some(&Value::Text(name.into()))
        })
        .unwrap_or_else(|| panic!("no {} {name}", kind.name()))
}

/// The ID of `document`'s paragraph style named `name`.
fn id(document: &Document, name: &str) -> ObjectId {
    named(document, doc::StyleKind::Paragraph, name)
}

/// The ID of `document`'s face named `name`.
fn face(document: &Document, name: &str) -> ObjectId {
    named(document, doc::StyleKind::Face, name)
}

#[test]
fn every_stylesheet_reads_and_has_what_documents_need() {
    let sheets = stylesheets();
    assert_eq!(sheets[0].0, "neutral", "new documents take Neutral");
    for (name, spec) in &sheets {
        let sheet = StyleSheet::from_spec(spec);
        // Markdown's styles, by role.
        for role in ["body", "heading-1", "heading-2", "heading-3", "code"] {
            assert!(
                sheet.by_role(role).is_some(),
                "{name} has a style for {role}"
            );
        }
        for level in 1..=6 {
            assert!(sheet.heading(level).is_some(), "{name} has Heading {level}");
        }
        assert!(
            sheet.code().is_some_and(|s| s.code),
            "{name}'s code is code"
        );
        // The marks, as character styles.
        for role in ["emphasis", "strong", "code", "link", "strikethrough"] {
            assert!(
                sheet
                    .character
                    .iter()
                    .any(|s| s.role.as_deref() == Some(role)),
                "{name} has a {role} mark"
            );
        }
        // Its faces and colours are parts (project owner, 2026-10-24,
        // review 30dmn1js), and its text names them.
        assert!(sheet.parts.iter().any(|p| p.kind == doc::StyleKind::Face));
        assert!(sheet.parts.iter().any(|p| p.kind == doc::StyleKind::Colour));
        let document = story::new_document(spec);
        let defaults = &document.stylesheet().unwrap().props;
        assert!(
            matches!(defaults.get(keys::FONT), Some(Value::Ref(_))),
            "{name}"
        );
        assert!(
            matches!(defaults.get(keys::COLOR), Some(Value::Ref(_))),
            "{name}"
        );
        assert_eq!(sheet.origin, Some((spec.name.clone(), spec.version)));
        assert!(sheet.page.body_width() > 0 && sheet.page.body_height() > 0);
    }
}

/// Neutral is M2's look from before documents held their own styles.
#[test]
fn neutral_is_m2s_look() {
    let sheet = StyleSheet::from_spec(&neutral());
    let body = sheet.default_style();
    assert_eq!(body.name, "Body");
    assert_eq!(
        (body.family.as_str(), body.size, body.space_after, body.bold),
        ("Source Serif 4", 11 * POINT, 6 * POINT, false)
    );
    let h1 = sheet.heading(1).unwrap();
    assert_eq!(
        (
            h1.family.as_str(),
            h1.size,
            h1.bold,
            h1.space_before,
            h1.space_after,
            h1.keep_with_next
        ),
        ("Inter", 20 * POINT, true, 18 * POINT, 6 * POINT, true)
    );
    let h4 = sheet.heading(4).unwrap();
    assert_eq!((h4.size, h4.space_before), (11 * POINT, 8 * POINT));
    let h6 = sheet.heading(6).unwrap();
    assert_eq!(
        (h6.size, h6.italic, h6.bold, h6.heading),
        (10 * POINT, true, true, Some(6))
    );
    let code = sheet.code().unwrap();
    assert_eq!(
        (code.family.as_str(), code.size, code.ligatures),
        ("JetBrains Mono", 19 * POINT / 2, text::Ligatures::NONE)
    );
    assert_eq!(
        (sheet.page.width, sheet.page.height, sheet.page.left),
        (210 * MILLIMETRE, 297 * MILLIMETRE, 25 * MILLIMETRE)
    );
    assert_eq!(sheet.tabs.interval, 25 * MILLIMETRE / 2);
    let bullets = sheet.list(false, None);
    assert_eq!(
        (bullets.bullets.clone(), bullets.indent),
        (vec!["•".to_string(), "–".to_string()], 18 * POINT)
    );
    assert_eq!(sheet.list(true, None).number(3), "3.");
    assert_eq!(sheet.blocks.code_tint, (243, 243, 240));
    assert_eq!(sheet.blocks.quote_color, (196, 196, 196));
    // Links in their colour, and inline code in the code face with no
    // ligatures, from the marks' character styles.
    let link = story::styles::MARKS
        .iter()
        .position(|(k, _)| *k == keys::LINK)
        .unwrap();
    assert_eq!(sheet.mark(link).color, Some((25, 80, 170)));
    let code = story::styles::MARKS
        .iter()
        .position(|(k, _)| *k == keys::CODE)
        .unwrap();
    assert_eq!(sheet.mark(code).family.as_deref(), Some("JetBrains Mono"));
}

/// A style sets only what differs from the style it's based on, so a
/// change to a base reaches every style based on it, at any depth.
#[test]
fn a_change_to_a_base_reaches_every_style_up_the_chain() {
    let spec = neutral();
    let mut document = story::new_document(&spec);
    let heading = id(&document, "Heading");
    document
        .apply(&Op::SetStyleProp {
            style: heading,
            key: keys::FONT.to_string(),
            value: Some(Value::Text("Noto Serif".to_string())),
        })
        .unwrap();
    let program = StyleSheet::from_spec(&spec);
    let sheet = StyleSheet::of(&document, &program);
    for level in 1..=6 {
        assert_eq!(
            sheet.heading(level).unwrap().family,
            "Noto Serif",
            "Heading {level}"
        );
    }
    // Heading 6 is based on Heading 5, which is based on Heading: a chain
    // of three, and Heading 5's size reaches Heading 6.
    let h5 = id(&document, "Heading 5");
    document
        .apply(&Op::SetStyleProp {
            style: h5,
            key: keys::SIZE.to_string(),
            value: Some(Value::Int(13 * POINT)),
        })
        .unwrap();
    let sheet = StyleSheet::of(&document, &program);
    assert_eq!(sheet.heading(6).unwrap().size, 13 * POINT);
    assert_eq!(sheet.default_style().family, "Source Serif 4");
}

/// Changing a part, such as the Text face, changes every style that uses
/// it, in one place, without basing every style on Body.
#[test]
fn a_part_changes_every_style_that_names_it() {
    let spec = neutral();
    let mut document = story::new_document(&spec);
    let text = face(&document, "Text");
    let link = named(&document, doc::StyleKind::Colour, "Link");
    document
        .apply_all(&[
            Op::SetStyleProp {
                style: text,
                key: keys::FAMILY.to_string(),
                value: Some(Value::Text("Noto Serif".to_string())),
            },
            Op::SetStyleProp {
                style: link,
                key: keys::COLOR.to_string(),
                value: Some(Value::Text("#aa0000".to_string())),
            },
        ])
        .unwrap();
    let sheet = StyleSheet::of(&document, &StyleSheet::from_spec(&spec));
    assert_eq!(sheet.default_style().family, "Noto Serif");
    assert_eq!(sheet.defaults.family, "Noto Serif");
    assert_eq!(
        sheet.heading(1).unwrap().family,
        "Inter",
        "headings have a face of their own"
    );
    let k = story::styles::MARKS
        .iter()
        .position(|(k, _)| *k == keys::LINK)
        .unwrap();
    assert_eq!(sheet.mark(k).color, Some((0xaa, 0, 0)));
}

/// Where each value comes from: the style itself, one up its chain, a part
/// it names, or the defaults.
#[test]
fn each_value_says_where_it_comes_from() {
    let document = story::new_document(&neutral());
    let h1 = id(&document, "Heading 1");
    let heading = id(&document, "Heading");
    let body = id(&document, "Body");
    let get = |style, key: &str| {
        find(&document, Some(style), key, true, &valid(key))
            .unwrap()
            .1
    };
    assert_eq!(get(h1, keys::SIZE).level, Level::Style(h1));
    assert_eq!(get(h1, keys::BOLD).level, Level::Style(heading));
    let face = get(h1, keys::FONT);
    assert_eq!(face.level, Level::Style(heading));
    assert_eq!(face.part, Some(self::face(&document, "Heading")));
    let from_defaults = get(body, keys::FONT);
    assert_eq!(from_defaults.level, Level::Defaults);
    assert_eq!(from_defaults.part, Some(self::face(&document, "Text")));
    assert_eq!(get(body, keys::SPACE_AFTER).level, Level::Style(body));
}

/// Marks are character styles: emphasis and strong look as the sheet
/// says, absolutely, so strong in a bold heading is bold, not undone.
#[test]
fn marks_are_character_styles() {
    let spec = neutral();
    let mut document = story::document(
        &spec,
        &[
            (
                "Heading 1",
                vec![
                    ("bold ", props(&[])),
                    ("strong", props(&[(keys::BOLD, Value::Bool(true))])),
                ],
            ),
            (
                "Body",
                vec![("emphasis", props(&[(keys::ITALIC, Value::Bool(true))]))],
            ),
        ],
    );
    let program = StyleSheet::from_spec(&spec);
    let spans = |document: &Document| -> Vec<(u16, bool)> {
        let sheet = StyleSheet::of(document, &program);
        document
            .paragraphs()
            .iter()
            .flat_map(|&p| story::spans(document.paragraph(p).unwrap(), &sheet))
            .map(|s| (s.style.weight, s.style.italic))
            .collect()
    };
    assert_eq!(spans(&document), [(700, false), (700, false), (400, true)]);
    // Emphasis made bold and upright instead: a value, not a toggle.
    let emphasis = document
        .styles()
        .iter()
        .copied()
        .find(|&s| {
            document.style(s).unwrap().props.get(keys::ROLE)
                == Some(&Value::Text("emphasis".into()))
        })
        .unwrap();
    document
        .apply_all(&[
            Op::SetStyleProp {
                style: emphasis,
                key: keys::ITALIC.to_string(),
                value: Some(Value::Bool(false)),
            },
            Op::SetStyleProp {
                style: emphasis,
                key: keys::BOLD.to_string(),
                value: Some(Value::Bool(true)),
            },
        ])
        .unwrap();
    assert_eq!(spans(&document), [(700, false), (700, false), (700, false)]);
}

/// A document's stylesheet written as a file reads back as the same
/// stylesheet, for every one that comes with xWorks.
#[test]
fn a_stylesheet_written_reads_back_the_same() {
    for (name, spec) in stylesheets() {
        let document = story::new_document(&spec);
        let written = SheetSpec::of(&document).expect("it writes").write();
        let again = SheetSpec::parse(&written).unwrap_or_else(|e| panic!("{name}: {e}\n{written}"));
        assert_eq!(
            StyleSheet::from_spec(&again),
            StyleSheet::from_spec(&spec),
            "{name}"
        );
    }
}

/// A quote's space above and below is its sheet's (review sjj5ak5a), kept
/// as the document's and written back as the sheet's; a sheet without it
/// sets quotes off with none, not with the program's.
#[test]
fn a_quotes_space_is_its_sheets_or_none() {
    let mut spec = neutral();
    assert_eq!(spec.document.quote_space, None, "Neutral has none");
    assert_eq!(StyleSheet::from_spec(&spec).blocks.quote_space, 0);
    spec.document.quote_space = Some(text::units::Length::Pt(7.0));
    let program = StyleSheet::from_spec(&spec);
    assert_eq!(program.blocks.quote_space, 7 * POINT);
    let document = story::new_document(&spec);
    assert_eq!(
        document.stylesheet().unwrap().props.get(keys::QUOTE_SPACE),
        Some(&Value::Int(7 * POINT))
    );
    let written = SheetSpec::of(&document).expect("it writes");
    assert_eq!(
        written.document.quote_space,
        Some(text::units::Length::Pt(7.0))
    );
    assert_eq!(SheetSpec::parse(&written.write()).unwrap(), written);
    // A document whose sheet lacks it, with a program's that has it.
    let without = story::new_document(&neutral());
    assert_eq!(StyleSheet::of(&without, &program).blocks.quote_space, 0);
}

/// A file from outside is checked: what doesn't hold together, or is past
/// a limit, is refused, and nothing panics.
#[test]
fn a_bad_stylesheet_is_refused() {
    let good = neutral().write();
    let refused = |text: &str| SheetSpec::parse(text).is_err();
    assert!(refused(""));
    assert!(refused("not RON at all"));
    assert!(refused("()"));
    let swap = |from: &str, to: &str| {
        assert!(good.contains(from), "{from}");
        good.replacen(from, to, 1)
    };
    // A style based on one that isn't there, or on itself.
    assert!(refused(&swap(
        "based_on: \"Heading 5\"",
        "based_on: \"Heading 9\""
    )));
    let mut looping = neutral();
    looping
        .paragraph
        .iter_mut()
        .find(|s| s.name == "Heading")
        .unwrap()
        .based_on = Some("Heading 1".to_string());
    assert!(matches!(looping.check(), Err(SheetError::Invalid(_))));
    // A face or a colour that isn't there; a colour that isn't one.
    assert!(refused(&swap("face: \"Code\"", "face: \"Mono\"")));
    assert!(refused(&swap("colour: \"#1950aa\"", "colour: \"blue\"")));
    // Sizes and margins past their limits.
    assert!(refused(&swap("size: Pt(11.0)", "size: Pt(100000.0)")));
    assert!(refused(&swap("left: Mm(25.0)", "left: Mm(500.0)")));
    let mut many = neutral();
    let body = many.paragraph[0].clone();
    for n in 0..600 {
        let mut s = body.clone();
        s.name = format!("Style {n}");
        s.role = None;
        many.paragraph.push(s);
    }
    assert!(many.check().is_err());
    // A quote's space past an inch.
    let mut spaced = neutral();
    spaced.document.quote_space = Some(text::units::Length::Pt(100.0));
    assert!(spaced.check().is_err());
    // Too large to read.
    assert!(refused(&" ".repeat(story::sheet::MOST_BYTES + 1)));
    // Bytes of any kind give an error or a stylesheet, never a panic.
    for cut in (0..good.len()).step_by(97) {
        if good.is_char_boundary(cut) {
            let _ = SheetSpec::parse(&good[..cut]);
        }
    }
}

/// The destination's styles win as pasting brings another document's in,
/// matched by role and name; those it lacks come along with their chains;
/// nothing is renamed "Body 2".
#[test]
fn merging_matches_by_role_and_name_and_brings_what_is_missing() {
    let spec = neutral();
    let mut theirs = story::new_document(&spec);
    let their_body = id(&theirs, "Body");
    let new_style = theirs.new_id();
    let deeper = theirs.new_id();
    let sheet = theirs.sheet().unwrap();
    let style = |base, name: &str| doc::Style {
        sheet,
        kind: doc::StyleKind::Paragraph,
        base: Some(base),
        props: Props::from([
            (keys::NAME.to_string(), Value::Text(name.to_string())),
            (keys::SIZE.to_string(), Value::Int(9 * POINT)),
        ]),
    };
    theirs
        .apply_all(&[
            Op::SetStyleProp {
                style: their_body,
                key: keys::SIZE.to_string(),
                value: Some(Value::Int(14 * POINT)),
            },
            Op::InsertStyle {
                sheet,
                after: None,
                id: new_style,
                style: style(their_body, "Caption"),
            },
            Op::InsertStyle {
                sheet,
                after: None,
                id: deeper,
                style: style(new_style, "Small Caption"),
            },
        ])
        .unwrap();
    let ours = story::new_document(&spec);
    let mut pasted = ours.clone();
    let (ops, map) = story::merge(&mut pasted, &theirs, false);
    // Body matched ours, which keeps its size; Caption and Small Caption
    // came along, Caption based on our Body.
    assert_eq!(map[&their_body], id(&ours, "Body"));
    let caption = map[&new_style];
    assert_eq!(pasted.style(caption).unwrap().base, Some(id(&ours, "Body")));
    assert_eq!(pasted.style(map[&deeper]).unwrap().base, Some(caption));
    let program = StyleSheet::from_spec(&spec);
    assert_eq!(
        StyleSheet::of(&pasted, &program).default_style().size,
        11 * POINT
    );
    let mut names: Vec<(doc::StyleKind, &Value)> = pasted
        .styles()
        .iter()
        .filter_map(|&s| {
            let style = pasted.style(s).unwrap();
            Some((style.kind, style.props.get(keys::NAME)?))
        })
        .collect();
    let all = names.len();
    names.sort_by_key(|(k, n)| (*k, format!("{n:?}")));
    names.dedup();
    assert_eq!(names.len(), all, "no style is there twice, nor renamed");
    // The operations make the same change to the document as it was.
    let mut again = ours.clone();
    again.apply_all(&ops).unwrap();
    assert_eq!(again, pasted);
    // Pasting again brings nothing new: everything matches now.
    let (more, _) = story::merge(&mut pasted, &theirs, false);
    assert!(more.is_empty());

    // Replacing the stylesheet: theirs wins, and our own styles stay.
    let mut replaced = ours.clone();
    story::merge(&mut replaced, &theirs, true);
    let sheet = StyleSheet::of(&replaced, &program);
    assert_eq!(sheet.default_style().size, 14 * POINT);
    assert!(sheet.paragraph.iter().any(|s| s.name == "Small Caption"));
    let changes = story::changes(&ours, &theirs);
    assert!(
        changes
            .iter()
            .any(|c| c.style == "Body" && c.what.contains("size 14 pt"))
    );
    assert!(
        changes
            .iter()
            .any(|c| c.style == "Caption" && c.what.starts_with("a new"))
    );
    assert!(story::changes(&replaced, &theirs).is_empty());
}

/// A technical document's headings are numbered: counted by level, a
/// heading starting its level's count again under each above it.
#[test]
fn numbered_headings_are_counted_by_level() {
    let spec = neutral();
    let mut document = story::document(
        &spec,
        &[
            ("Heading 1", vec![("One", props(&[]))]),
            ("Heading 2", vec![("One one", props(&[]))]),
            ("Heading 2", vec![("One two", props(&[]))]),
            ("Body", vec![("Text", props(&[]))]),
            ("Heading 1", vec![("Two", props(&[]))]),
            ("Heading 2", vec![("Two one", props(&[]))]),
        ],
    );
    let heading = id(&document, "Heading");
    document
        .apply(&Op::SetStyleProp {
            style: heading,
            key: keys::NUMBERED.to_string(),
            value: Some(Value::Bool(true)),
        })
        .unwrap();
    let sheet = StyleSheet::of(&document, &StyleSheet::from_spec(&spec));
    let markers: Vec<Option<String>> = story::flow(&document, &sheet, 100 * MILLIMETRE)
        .entries
        .iter()
        .map(|e| e.place.marker.clone())
        .collect();
    assert_eq!(
        markers,
        [
            Some("1.".to_string()),
            Some("1.1.".to_string()),
            Some("1.2.".to_string()),
            None,
            Some("2.".to_string()),
            Some("2.1.".to_string()),
        ]
    );
}

/// Containers lend geometry: in a sheet whose body text is justified and
/// indented, as Book's is, a table's cells sit at the start of their
/// columns without the indent, and an item's first line starts at its
/// marker; a paragraph in neither keeps both.
#[test]
fn containers_lend_their_geometry() {
    let book = stylesheets()
        .into_iter()
        .find(|(n, _)| n == "book")
        .expect("Book")
        .1;
    let body =
        |text: &str| story::BlockSpec::Paragraph("Body".into(), vec![(text.into(), Props::new())]);
    let document = story::build(
        &book,
        &[
            body("Plain"),
            story::BlockSpec::List(false, vec![vec![body("Item")]]),
            story::BlockSpec::Table(
                vec![String::new()],
                vec![vec![vec![("Cell".into(), Props::new())]]],
            ),
        ],
    );
    let sheet = StyleSheet::from_spec(&book);
    let flow = story::flow(&document, &sheet, 100 * MILLIMETRE);
    let at = |k: usize| {
        let entry = &flow.entries[k];
        let p = document.paragraph(entry.id).unwrap();
        (
            story::indents_at(p, &sheet, &entry.place).first,
            story::align_at(p, &sheet, &entry.place),
        )
    };
    let indent = sheet.default_style().indents.first;
    assert!(indent > 0);
    assert_eq!(at(0), (indent, text::Align::Justify));
    assert_eq!(at(1), (0, text::Align::Justify));
    assert_eq!(at(2), (0, text::Align::Start));
}
