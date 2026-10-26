//! Taking a newer version of a document's stylesheet, three-way (review
//! sjj5ak5a): from the version the document copied, kept in
//! `spec/styles/versions/`, to the newer one, keeping what the document
//! changed itself.

use doc::{Document, ObjectId, Op, Value};
use story::{SheetSpec, StyleSheet, Update, keys};
use text::units::{Length, MILLIMETRE, POINT};

/// The stylesheet that comes with xWorks in `spec/styles/NAME.ron`, or
/// a version of it kept in `spec/styles/versions/`.
fn spec(dir: &str, name: &str) -> SheetSpec {
    let text = testkit::specs(dir)
        .expect("spec/")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("spec/{dir}/{name}.ron"))
        .1;
    SheetSpec::parse(&text).expect("a stylesheet")
}

fn book_1() -> SheetSpec {
    spec("styles/versions", "book-1")
}

fn book_2() -> SheetSpec {
    let book = spec("styles", "book");
    assert_eq!(book.version, 2, "Book's version 2 is this follow-up's");
    book
}

/// The ID of `document`'s paragraph style named `name`.
fn id(document: &Document, name: &str) -> ObjectId {
    story::style_named(document, name).unwrap_or_else(|| panic!("no style {name}"))
}

/// `document` with `style`'s `key` set to `value`.
fn set(document: &mut Document, style: ObjectId, key: &str, value: Value) {
    document
        .apply(&Op::SetStyleProp {
            style,
            key: key.to_string(),
            value: Some(value),
        })
        .expect("it applies");
}

/// What updating `document`, copied from `base`, to `newer` does, and the
/// document after.
fn updated(document: &Document, base: &SheetSpec, newer: &SheetSpec) -> (Update, Document) {
    let mut after = document.clone();
    let update = story::update(
        &mut after,
        Some(&story::new_document(base)),
        &story::new_document(newer),
    );
    // Its operations make the same of the document as it was.
    let mut again = document.clone();
    again.apply_all(&update.ops).expect("they apply");
    assert_eq!(again.to_snapshot(), after.to_snapshot());
    (update, after)
}

fn said(changes: &[story::Change]) -> Vec<String> {
    changes
        .iter()
        .map(|c| format!("{}: {}", c.style, c.what))
        .collect()
}

/// A document copied from Book 1, its Body changed, takes Book 2: it gains
/// Table Header and the quote's space, keeps its Body as it changed it,
/// and records Book 2 as where it came from.
#[test]
fn a_document_from_book_1_takes_book_2_and_keeps_its_own_changes() {
    let mut document = story::new_document(&book_1());
    let body = id(&document, "Body");
    set(&mut document, body, keys::SIZE, Value::Int(12 * POINT));
    let (update, after) = updated(&document, &book_1(), &book_2());
    assert!(update.three_way);
    assert!(update.kept.is_empty(), "{:?}", said(&update.kept));
    let sheet = StyleSheet::from_spec(&book_2());
    let resolved = StyleSheet::of(&after, &sheet);
    assert_eq!(resolved.default_style().size, 12 * POINT, "its own change");
    let header = resolved.table_header().expect("Table Header");
    assert!(header.italic);
    assert_eq!(header.base, Some(body));
    assert_eq!(resolved.blocks.quote_space, Length::Pt(7.2).units());
    assert_eq!(resolved.origin, Some(("Book".to_string(), 2)));
    // Table Header comes after Code, before Heading, as Book 2 has it.
    let names: Vec<&str> = resolved.paragraph.iter().map(|s| s.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|n| *n == name).unwrap();
    assert_eq!(at("Table Header"), at("Code") + 1);
    assert_eq!(at("Heading"), at("Table Header") + 1);
    // Only what Book 2 changed is said, and not Body.
    let said = said(&update.changes);
    assert_eq!(
        said,
        [
            "Defaults: quote-space 7.2 pt, not nothing",
            "Table Header: a new paragraph style",
        ],
        "{said:?}"
    );
}

/// Where the document and the newer version changed the same value
/// differently, the document's stays and that's said; where they made the
/// same change, nothing is.
#[test]
fn where_both_changed_a_value_the_documents_stays_and_is_said() {
    let mut newer = book_2();
    newer.version = 3;
    let body = newer
        .paragraph
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap();
    body.indent_first = Some(Length::Mm(5.0));
    body.size = Some(Length::Pt(12.0));
    let mut document = story::new_document(&book_2());
    let body = id(&document, "Body");
    set(
        &mut document,
        body,
        keys::INDENT_FIRST,
        Value::Int(6 * MILLIMETRE),
    );
    set(&mut document, body, keys::SIZE, Value::Int(12 * POINT));
    let (update, after) = updated(&document, &book_2(), &newer);
    let resolved = StyleSheet::of(&after, &StyleSheet::from_spec(&newer));
    let body = resolved.default_style();
    assert_eq!(body.indents.first, 6 * MILLIMETRE, "the document's stays");
    assert_eq!(body.size, 12 * POINT);
    assert_eq!(
        said(&update.kept),
        ["Body: indent-first 17.01 pt, kept as the document has it; the new version has 14.17 pt"]
    );
    assert!(update.changes.is_empty(), "{:?}", said(&update.changes));
    assert_eq!(resolved.origin, Some(("Book".to_string(), 3)));
}

/// A style the newer version removes goes, if nothing uses it and the
/// document didn't change it; otherwise it stays, and that's said.
#[test]
fn a_style_the_newer_version_removes_goes_only_if_unused_and_unchanged() {
    let mut newer = book_2();
    newer.version = 3;
    newer.paragraph.retain(|s| s.name != "Heading 6");
    // Nothing uses it: it goes.
    let document = story::new_document(&book_2());
    let (update, after) = updated(&document, &book_2(), &newer);
    assert!(story::style_named(&after, "Heading 6").is_none());
    assert_eq!(said(&update.changes), ["Heading 6: removed"]);
    // A paragraph in it: it stays.
    let mut used = document.clone();
    let first = used.paragraphs()[0];
    let heading = id(&used, "Heading 6");
    used.apply(&Op::SetParaProp {
        paragraph: first,
        key: keys::STYLE.to_string(),
        value: Some(Value::Ref(heading)),
    })
    .unwrap();
    let (update, after) = updated(&used, &book_2(), &newer);
    assert!(story::style_named(&after, "Heading 6").is_some());
    assert_eq!(
        said(&update.kept),
        ["Heading 6: removed from the new version, kept as the document uses it"]
    );
    // Changed in the document: it stays.
    let mut changed = document.clone();
    let heading = id(&changed, "Heading 6");
    set(&mut changed, heading, keys::ITALIC, Value::Bool(true));
    let (update, after) = updated(&changed, &book_2(), &newer);
    assert!(story::style_named(&after, "Heading 6").is_some());
    assert_eq!(
        said(&update.kept),
        ["Heading 6: removed from the new version, kept as the document changed it"]
    );
}

/// A style the newer version bases on another is based on it in the
/// document, unless the document based it otherwise itself.
#[test]
fn a_new_base_is_taken_unless_the_document_changed_it() {
    let mut newer = book_2();
    newer.version = 3;
    newer
        .paragraph
        .iter_mut()
        .find(|s| s.name == "Heading 6")
        .unwrap()
        .based_on = Some("Heading 3".to_string());
    let document = story::new_document(&book_2());
    let (update, after) = updated(&document, &book_2(), &newer);
    let style = |document: &Document, name: &str| document.style(id(document, name)).cloned();
    assert_eq!(
        style(&after, "Heading 6").unwrap().base,
        Some(id(&after, "Heading 3"))
    );
    assert_eq!(said(&update.changes), ["Heading 6: based on Heading 3"]);
    let mut rebased = document.clone();
    let (h6, h4) = (id(&rebased, "Heading 6"), id(&rebased, "Heading 4"));
    rebased
        .apply(&Op::SetBase {
            style: h6,
            base: Some(h4),
        })
        .unwrap();
    let (update, after) = updated(&rebased, &book_2(), &newer);
    assert_eq!(style(&after, "Heading 6").unwrap().base, Some(h4));
    assert_eq!(
        said(&update.kept),
        ["Heading 6: based on Heading 4, kept as the document has it"]
    );
}

/// A style the newer version adds that the document has already, as a
/// schema 2 document's upgrade gives it Table Header, isn't a change
/// where the two agree.
#[test]
fn a_style_both_added_alike_is_no_change() {
    let mut document = story::new_document(&spec("styles/versions", "neutral-2"));
    let neutral = spec("styles", "neutral");
    let header = neutral
        .paragraph
        .iter()
        .find(|s| s.role.as_deref() == Some(story::TABLE_HEADER))
        .unwrap()
        .clone();
    let sheet = document.sheet().unwrap();
    let new = document.new_id();
    let body = id(&document, "Body");
    let mut props = doc::Props::new();
    props.insert(keys::NAME.to_string(), Value::Text(header.name.clone()));
    props.insert(
        keys::ROLE.to_string(),
        Value::Text(story::TABLE_HEADER.to_string()),
    );
    props.insert(keys::BOLD.to_string(), Value::Bool(true));
    document
        .apply(&Op::InsertStyle {
            sheet,
            after: None,
            id: new,
            style: doc::Style {
                sheet,
                kind: doc::StyleKind::Paragraph,
                base: Some(body),
                props,
            },
        })
        .unwrap();
    let (update, _) = updated(&document, &spec("styles/versions", "neutral-2"), &neutral);
    assert!(update.kept.is_empty(), "{:?}", said(&update.kept));
    assert!(update.changes.is_empty(), "{:?}", said(&update.changes));
}

/// Without the version the document copied, the update compares the
/// document's sheet with the newer one alone, two ways, the newer one's
/// winning, and says so: a change of the document's own is taken back.
#[test]
fn without_its_base_an_update_is_two_way() {
    let mut document = story::new_document(&book_1());
    let body = id(&document, "Body");
    set(&mut document, body, keys::SIZE, Value::Int(12 * POINT));
    let newer = story::new_document(&book_2());
    let update = story::update(&mut document, None, &newer);
    assert!(!update.three_way);
    let resolved = StyleSheet::of(&document, &StyleSheet::from_spec(&book_2()));
    assert_eq!(resolved.default_style().size, Length::Pt(11.5).units());
    assert!(
        said(&update.changes)
            .iter()
            .any(|c| c.starts_with("Body: size")),
        "{:?}",
        said(&update.changes)
    );
    assert_eq!(resolved.origin, Some(("Book".to_string(), 2)));
}

/// Every kept version of every sheet updates to its current one, three
/// ways, as nothing of the document's is its own: to the same sheet as
/// the current one makes, and saying only what changed.
#[test]
fn every_kept_version_updates_to_the_current_one() {
    for (file, text) in testkit::specs("styles/versions").expect("spec/styles/versions") {
        let old = SheetSpec::parse(&text).expect("a stylesheet");
        let stem = file.rsplit_once('-').unwrap().0;
        let current = spec("styles", stem);
        let document = story::new_document(&old);
        let (update, after) = updated(&document, &old, &current);
        assert!(update.kept.is_empty(), "{file}: {:?}", said(&update.kept));
        assert!(!update.changes.is_empty(), "{file}");
        // Written as a file, it's the current sheet, in its order.
        let written = SheetSpec::of(&after).expect("it writes");
        assert_eq!(
            StyleSheet::from_spec(&written),
            StyleSheet::from_spec(&current),
            "{file}"
        );
    }
}
