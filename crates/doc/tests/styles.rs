//! A document's own stylesheet (M2 stage 9; docs/ARCHITECTURE.md, section
//! 6): styles and named parts are objects, made, changed and deleted by
//! operations; a style is based on one of its kind, never on itself; and
//! paragraphs name styles by ID, so renaming one is one edit.

use doc::{
    Document, Error, Object, ObjectId, Op, Paragraph, Props, Sheet, Snapshot, SnapshotError, Story,
    Style, StyleKind, Value, rebase,
};

const SHEET: ObjectId = ObjectId(2);
const BODY: ObjectId = ObjectId(3);
const QUOTE: ObjectId = ObjectId(4);
const NOTE: ObjectId = ObjectId(5);
const STRONG: ObjectId = ObjectId(6);
const TEXT: ObjectId = ObjectId(7);

fn named(name: &str) -> Props {
    Props::from([("name".to_string(), Value::Text(name.to_string()))])
}

fn style(kind: StyleKind, base: Option<ObjectId>, name: &str) -> Style {
    Style {
        sheet: SHEET,
        kind,
        base,
        props: named(name),
    }
}

/// A story of one paragraph in Body, and a stylesheet: Body; Quote based
/// on Body; Note based on Quote; Strong, a character style; and Text, a
/// face.
fn snapshot() -> Snapshot {
    let mut props = Props::new();
    props.insert("style".into(), Value::Ref(BODY));
    let styles = [
        (BODY, style(StyleKind::Paragraph, None, "Body")),
        (QUOTE, style(StyleKind::Paragraph, Some(BODY), "Quote")),
        (NOTE, style(StyleKind::Paragraph, Some(QUOTE), "Note")),
        (STRONG, style(StyleKind::Character, None, "Strong")),
        (TEXT, style(StyleKind::Face, None, "Text")),
    ];
    let mut objects = vec![
        (
            ObjectId(0),
            Object::Story(Story {
                blocks: vec![ObjectId(1)],
            }),
        ),
        (
            ObjectId(1),
            Object::Paragraph(Paragraph {
                parent: ObjectId(0),
                text: String::new(),
                runs: Vec::new(),
                props,
            }),
        ),
        (
            SHEET,
            Object::Sheet(Sheet {
                styles: styles.iter().map(|(id, _)| *id).collect(),
                props: Props::from([("size".to_string(), Value::Int(11))]),
            }),
        ),
    ];
    objects.extend(styles.into_iter().map(|(id, s)| (id, Object::Style(s))));
    Snapshot {
        root: ObjectId(0),
        next_id: 8,
        objects,
    }
}

fn document() -> Document {
    Document::from_snapshot(snapshot()).expect("it holds together")
}

fn base(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    doc.style(id).expect("a style").base
}

#[test]
fn a_stylesheet_reads_from_its_snapshot() {
    let doc = document();
    assert_eq!(doc.sheet(), Some(SHEET));
    assert_eq!(doc.styles(), [BODY, QUOTE, NOTE, STRONG, TEXT]);
    assert_eq!(doc.chain(NOTE), [NOTE, QUOTE, BODY]);
    assert_eq!(doc.paragraphs(), [ObjectId(1)]);
    assert_eq!(Document::new().sheet(), None);
    let dump = doc.dump();
    assert!(
        dump.contains("#2 sheet [#3, #4, #5, #6, #7] {\"size\": Int(11)}"),
        "{dump}"
    );
    assert!(dump.contains("#5 paragraph style in #2 based on #4 {\"name\": Text(\"Note\")}"));
    assert!(dump.contains("#7 face in #2 {\"name\": Text(\"Text\")}"));
}

#[test]
fn renaming_a_style_is_one_edit_that_paragraphs_never_notice() {
    let mut doc = document();
    let paragraph = doc.paragraph(ObjectId(1)).expect("a paragraph").clone();
    let undo = doc
        .apply(&Op::SetStyleProp {
            style: BODY,
            key: "name".into(),
            value: Some(Value::Text("Text Body".into())),
        })
        .expect("it applies");
    assert_eq!(
        doc.style(BODY).unwrap().props.get("name"),
        Some(&Value::Text("Text Body".into()))
    );
    assert_eq!(doc.paragraph(ObjectId(1)), Some(&paragraph));
    doc.apply_all(&undo).expect("it undoes");
    assert_eq!(doc, document());
}

#[test]
fn styles_are_made_based_and_deleted_and_undo_exactly() {
    let mut doc = document();
    let new = doc.new_id();
    let ops = [
        Op::InsertStyle {
            sheet: SHEET,
            after: Some(QUOTE),
            id: new,
            style: style(StyleKind::Paragraph, Some(QUOTE), "Epigraph"),
        },
        Op::SetBase {
            style: NOTE,
            base: Some(new),
        },
        Op::SetStyleProp {
            style: SHEET,
            key: "size".into(),
            value: Some(Value::Int(12)),
        },
        Op::SetBase {
            style: NOTE,
            base: None,
        },
        Op::DeleteStyle { style: new },
    ];
    let mut states = vec![doc.clone()];
    let mut undos = Vec::new();
    for op in &ops {
        undos.push(doc.apply(op).unwrap_or_else(|e| panic!("{op:?}: {e}")));
        states.push(doc.clone());
    }
    assert_eq!(doc.styles(), [BODY, QUOTE, NOTE, STRONG, TEXT]);
    assert_eq!(base(&doc, NOTE), None);
    assert_eq!(
        doc.stylesheet().unwrap().props.get("size"),
        Some(&Value::Int(12))
    );
    for (undo, state) in undos.iter().rev().zip(states.iter().rev().skip(1)) {
        doc.apply_all(undo).expect("it undoes");
        assert_eq!(&doc, state);
    }
    // The epigraph came after Quote.
    let mut made = document();
    made.apply(&ops[0]).unwrap();
    assert_eq!(made.styles(), [BODY, QUOTE, new, NOTE, STRONG, TEXT]);
}

#[test]
fn bases_that_would_loop_and_other_bad_edits_are_refused() {
    let doc = document();
    let cases = [
        (
            Op::SetBase {
                style: BODY,
                base: Some(NOTE),
            },
            Error::Loop {
                style: BODY,
                base: NOTE,
            },
        ),
        (
            Op::SetBase {
                style: QUOTE,
                base: Some(QUOTE),
            },
            Error::Loop {
                style: QUOTE,
                base: QUOTE,
            },
        ),
        (
            Op::SetBase {
                style: QUOTE,
                base: Some(STRONG),
            },
            Error::WrongBase {
                style: QUOTE,
                base: STRONG,
            },
        ),
        (
            Op::SetBase {
                style: TEXT,
                base: Some(TEXT),
            },
            Error::WrongBase {
                style: TEXT,
                base: TEXT,
            },
        ),
        (
            Op::SetBase {
                style: ObjectId(1),
                base: None,
            },
            Error::NoSuchStyle(ObjectId(1)),
        ),
        (
            Op::DeleteStyle { style: QUOTE },
            Error::BasedOn {
                style: QUOTE,
                by: NOTE,
            },
        ),
        (Op::DeleteStyle { style: SHEET }, Error::NoSuchStyle(SHEET)),
        (
            Op::InsertStyle {
                sheet: SHEET,
                after: None,
                id: BODY,
                style: style(StyleKind::Paragraph, None, "Again"),
            },
            Error::IdInUse(BODY),
        ),
        (
            Op::InsertStyle {
                sheet: SHEET,
                after: None,
                id: ObjectId(20),
                style: style(StyleKind::Face, Some(TEXT), "Heading"),
            },
            Error::WrongBase {
                style: ObjectId(20),
                base: TEXT,
            },
        ),
        (
            Op::InsertStyle {
                sheet: ObjectId(0),
                after: None,
                id: ObjectId(20),
                style: style(StyleKind::Paragraph, None, "Elsewhere"),
            },
            Error::NoSuchStyle(ObjectId(0)),
        ),
        (
            Op::SetStyleProp {
                style: ObjectId(1),
                key: "size".into(),
                value: None,
            },
            Error::NoSuchStyle(ObjectId(1)),
        ),
        // Operations on blocks never touch the stylesheet.
        (Op::DeleteBlock { block: QUOTE }, Error::NoSuchBlock(QUOTE)),
        (Op::DeleteBlock { block: SHEET }, Error::NoSuchBlock(SHEET)),
        (
            Op::MoveBlock {
                block: ObjectId(1),
                parent: SHEET,
                after: None,
            },
            Error::NoSuchBlock(SHEET),
        ),
        (
            Op::InsertBlock {
                parent: SHEET,
                after: None,
                objects: vec![(
                    ObjectId(20),
                    Object::Style(style(StyleKind::Paragraph, Some(QUOTE), "Smuggled")),
                )],
            },
            Error::NoSuchBlock(SHEET),
        ),
    ];
    for (op, error) in cases {
        let mut d = doc.clone();
        assert_eq!(d.apply(&op), Err(error), "{op:?}");
        assert_eq!(d, doc, "{op:?} changed nothing");
    }
}

/// `first` then `second` rebased over it.
fn both(doc: &Document, first: &Op, second: &Op) -> Document {
    let mut d = doc.clone();
    d.apply(first).unwrap();
    d.apply_all(&rebase(doc, second, first, false)).unwrap();
    d
}

#[test]
fn of_two_bases_that_together_loop_the_one_rebased_later_is_dropped() {
    let doc = document();
    // Body on Note, after Note is based on nothing, and Note on Body: each
    // fine alone, a loop together.
    let mut doc2 = doc.clone();
    doc2.apply(&Op::SetBase {
        style: NOTE,
        base: None,
    })
    .unwrap();
    let a = Op::SetBase {
        style: BODY,
        base: Some(NOTE),
    };
    let b = Op::SetBase {
        style: NOTE,
        base: Some(BODY),
    };
    let ab = both(&doc2, &a, &b);
    assert_eq!((base(&ab, BODY), base(&ab, NOTE)), (Some(NOTE), None));
    let ba = both(&doc2, &b, &a);
    assert_eq!((base(&ba, BODY), base(&ba, NOTE)), (None, Some(BODY)));
    // And whichever order the server gave them, the first wins: rebased
    // over each other, both orders meet.
    let mut right = doc2.clone();
    right.apply(&b).unwrap();
    right.apply_all(&rebase(&doc2, &a, &b, true)).unwrap();
    assert_eq!(right, ab);
}

#[test]
fn the_later_of_two_settings_of_a_style_wins_and_a_deleted_style_takes_them_with_it() {
    let doc = document();
    let set = |style, size| Op::SetStyleProp {
        style,
        key: "size".into(),
        value: Some(Value::Int(size)),
    };
    let d = both(&doc, &set(NOTE, 10), &set(NOTE, 14));
    assert_eq!(
        d.style(NOTE).unwrap().props.get("size"),
        Some(&Value::Int(14))
    );
    let delete = Op::DeleteStyle { style: NOTE };
    let d = both(&doc, &delete, &set(NOTE, 14));
    assert_eq!(d.style(NOTE), None);
    let d = both(&doc, &set(NOTE, 14), &delete);
    assert_eq!(d.style(NOTE), None);
    // Styles put in one place: the first one's comes first.
    let insert = |id: u64, name: &str| Op::InsertStyle {
        sheet: SHEET,
        after: Some(BODY),
        id: ObjectId(id),
        style: style(StyleKind::Paragraph, None, name),
    };
    let d = both(&doc, &insert(20, "One"), &insert(21, "Two"));
    assert_eq!(&d.styles()[..3], [BODY, ObjectId(20), ObjectId(21)]);
}

#[test]
fn broken_stylesheets_are_refused() {
    let good = snapshot();
    let style_mut = |s: &mut Snapshot, id: ObjectId| -> Style {
        match &s.objects.iter().find(|(i, _)| *i == id).unwrap().1 {
            Object::Style(st) => st.clone(),
            _ => panic!("a style"),
        }
    };
    let replace = |s: &mut Snapshot, id: ObjectId, object: Object| {
        s.objects.iter_mut().find(|(i, _)| *i == id).unwrap().1 = object;
    };

    let mut s = good.clone();
    let mut body = style_mut(&mut s, BODY);
    body.base = Some(NOTE);
    replace(&mut s, BODY, Object::Style(body));
    assert!(matches!(
        Document::from_snapshot(s),
        Err(SnapshotError::Loop(_))
    ));

    let mut s = good.clone();
    let mut quote = style_mut(&mut s, QUOTE);
    quote.base = Some(STRONG);
    replace(&mut s, QUOTE, Object::Style(quote));
    assert_eq!(
        Document::from_snapshot(s),
        Err(SnapshotError::WrongBase {
            style: QUOTE,
            base: STRONG
        })
    );

    let mut s = good.clone();
    s.objects.push((
        ObjectId(9),
        Object::Sheet(Sheet {
            styles: Vec::new(),
            props: Props::new(),
        }),
    ));
    s.next_id = 10;
    assert_eq!(
        Document::from_snapshot(s),
        Err(SnapshotError::TwoSheets(ObjectId(9)))
    );

    // A style the stylesheet doesn't list.
    let mut s = good.clone();
    replace(
        &mut s,
        SHEET,
        Object::Sheet(Sheet {
            styles: vec![BODY, QUOTE, NOTE, STRONG],
            props: Props::new(),
        }),
    );
    assert_eq!(Document::from_snapshot(s), Err(SnapshotError::Stray(TEXT)));

    // A style in the story.
    let mut s = good.clone();
    replace(
        &mut s,
        ObjectId(0),
        Object::Story(Story {
            blocks: vec![ObjectId(1), TEXT],
        }),
    );
    assert!(Document::from_snapshot(s).is_err());
}
