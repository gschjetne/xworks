//! Properties of the document core, from random documents and random edits
//! (docs/ROADMAP.md, M1 stage 4):
//!
//! - every operation's inverse undoes it;
//! - rebasing converges: `a` then `b` rebased over it gives the same
//!   document as `b` then `a` rebased over it, for every pair of kinds;
//! - a snapshot is unchanged by later edits;
//! - a transaction applies all or nothing, and undo and redo are exact;
//! - operations survive serialization.

use doc::{Document, History, ObjectId, Op, Props, Run, Snapshot, Value, rebase};
use proptest::prelude::*;

/// Characters of one to four bytes, so character offsets and byte offsets
/// differ.
const ALPHABET: &[char] = &['a', 'b', ' ', 'é', 'ж', '中', '😀'];

/// Draws numbers from proptest's choices, cycling if they run out.
struct Choices<'a> {
    values: &'a [u32],
    at: usize,
}

impl Choices<'_> {
    fn next(&mut self) -> u32 {
        let v = self.values[self.at % self.values.len()];
        self.at += 1;
        v
    }

    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// An offset in `0..=n`, often at the start, the end or the middle,
    /// so that concurrent operations meet at the same place.
    fn offset(&mut self, n: u32) -> u32 {
        match self.below(4) {
            0 => 0,
            1 => n,
            2 => n / 2,
            _ => self.below(n + 1),
        }
    }

    fn text(&mut self, max: u32) -> String {
        let n = self.below(max + 1);
        (0..n)
            .map(|_| ALPHABET[self.below(ALPHABET.len() as u32) as usize])
            .collect()
    }

    fn props(&mut self) -> Props {
        let mut props = Props::new();
        if self.below(2) == 0 {
            props.insert("bold".into(), Value::Bool(true));
        }
        if self.below(3) == 0 {
            props.insert("size".into(), Value::Int(self.below(4) as i64 * 2 + 10));
        }
        props
    }

    fn value(&mut self, key: &str) -> Option<Value> {
        match (self.below(4), key) {
            (0, _) => None,
            (_, "bold" | "italic") => Some(Value::Bool(self.below(2) == 0)),
            (_, "size") => Some(Value::Int(self.below(4) as i64 + 9)),
            (v, _) => Some(Value::Text(
                ["Body", "Heading 1", "Quote"][v as usize % 3].into(),
            )),
        }
    }
}

fn len(doc: &Document, p: ObjectId) -> u32 {
    doc.paragraph(p).unwrap().len()
}

/// A paragraph, often the first, so that concurrent operations meet.
fn pick(doc: &Document, c: &mut Choices<'_>) -> ObjectId {
    let ps = doc.paragraphs();
    if c.below(2) == 0 {
        ps[0]
    } else {
        ps[c.below(ps.len() as u32) as usize]
    }
}

fn insert(doc: &Document, c: &mut Choices<'_>) -> Op {
    let paragraph = pick(doc, c);
    let mut text = c.text(4);
    if text.is_empty() {
        text.push('x');
    }
    let n = text.chars().count() as u32;
    let runs = vec![Run::new(n, c.props())];
    Op::InsertText {
        paragraph,
        offset: c.offset(len(doc, paragraph)),
        text,
        runs,
    }
}

/// A random operation that's valid on `doc`. New IDs come from `ids`.
fn op(doc: &Document, ids: &mut u64, c: &mut Choices<'_>) -> Op {
    let paragraph = pick(doc, c);
    let n = len(doc, paragraph);
    match c.below(6) {
        0 => insert(doc, c),
        1 if n > 0 => {
            let offset = c.offset(n - 1);
            Op::DeleteText {
                paragraph,
                offset,
                len: 1 + c.below(n - offset),
            }
        }
        2 => {
            *ids += 1;
            Op::SplitParagraph {
                paragraph,
                offset: c.offset(n),
                new: ObjectId(*ids),
                new_props: c.props(),
            }
        }
        3 if doc.paragraphs().len() > 1 => {
            let ps = doc.paragraphs();
            let i = c.below(ps.len() as u32 - 1) as usize;
            Op::JoinParagraphs {
                paragraph: ps[i],
                offset: len(doc, ps[i]),
                next: ps[i + 1],
            }
        }
        4 => {
            let offset = c.offset(n);
            let key = ["bold", "italic", "size"][c.below(3) as usize];
            Op::SetCharProp {
                paragraph,
                offset,
                len: c.below(n - offset + 1),
                key: key.into(),
                value: c.value(key),
            }
        }
        5 => {
            let key = ["style", "align"][c.below(2) as usize];
            Op::SetParaProp {
                paragraph,
                key: key.into(),
                value: c.value(key),
            }
        }
        _ => insert(doc, c),
    }
}

/// A random document: up to five paragraphs of formatted text in many
/// scripts, made by applying random operations to a new one.
fn document(c: &mut Choices<'_>, ids: &mut u64) -> Document {
    let mut doc = Document::new();
    let steps = 4 + c.below(16);
    for _ in 0..steps {
        let op = if doc.paragraphs().len() >= 5 && c.below(2) == 0 {
            insert(&doc, c)
        } else {
            op(&doc, ids, c)
        };
        doc.apply(&op).expect("a generated operation applies");
    }
    doc
}

fn choices() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(any::<u32>(), 64..256)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn inverses_restore_the_document(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let original = document(c, &mut ids);
        let mut doc = original.clone();
        let mut inverses = Vec::new();
        for _ in 0..1 + c.below(8) {
            let op = op(&doc, &mut ids, c);
            let before = doc.clone();
            let inverse = doc.apply(&op).expect("a generated operation applies");
            let mut undone = doc.clone();
            undone.apply_all(&inverse).expect("an inverse applies");
            prop_assert_eq!(&undone, &before, "{:?} undone by {:?}", op, inverse);
            inverses.push(inverse);
        }
        for inverse in inverses.iter().rev() {
            doc.apply_all(inverse).expect("an inverse applies");
        }
        prop_assert_eq!(doc, original);
    }

    #[test]
    fn rebasing_converges(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = document(c, &mut ids);
        let a = op(&doc, &mut ids, c);
        let b = op(&doc, &mut ids, c);

        let mut left = doc.clone();
        left.apply(&a).expect("a applies");
        let b_over_a = rebase(&b, &a, false);
        for x in &b_over_a {
            left.apply(x).map_err(|e| TestCaseError::fail(format!("b over a: {x:?}: {e}\na {a:?}\nb {b:?}\n{doc:?}")))?;
        }

        let mut right = doc.clone();
        right.apply(&b).expect("b applies");
        let a_over_b = rebase(&a, &b, true);
        for x in &a_over_b {
            right.apply(x).map_err(|e| TestCaseError::fail(format!("a over b: {x:?}: {e}\na {a:?}\nb {b:?}\n{doc:?}")))?;
        }

        prop_assert_eq!(&left, &right, "a {:?}\nb {:?}\nb over a {:?}\na over b {:?}\nfrom\n{:?}", a, b, b_over_a, a_over_b, doc);
    }

    #[test]
    fn snapshots_never_change(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let mut doc = document(c, &mut ids);
        let snapshot = doc.clone();
        let dump = snapshot.dump();
        for _ in 0..1 + c.below(8) {
            let op = op(&doc, &mut ids, c);
            doc.apply(&op).expect("a generated operation applies");
        }
        prop_assert_eq!(snapshot.dump(), dump);
    }

    #[test]
    fn documents_survive_storing_as_snapshots(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let mut doc = document(c, &mut ids);
        for _ in 0..c.below(6) {
            let op = op(&doc, &mut ids, c);
            doc.apply(&op).expect("a generated operation applies");
        }
        let text = ron::to_string(&doc.to_snapshot()).expect("a snapshot serializes");
        let back: Snapshot = ron::from_str(&text).expect("and reads back");
        let restored = Document::from_snapshot(back).expect("a snapshot of a document is valid");
        prop_assert_eq!(restored.dump(), doc.dump());
        prop_assert_eq!(&restored, &doc);
        // IDs given out after it don't collide with the stored ones.
        let mut again = restored.clone();
        let mut original = doc.clone();
        prop_assert_eq!(again.new_id(), original.new_id());
    }

    #[test]
    fn transactions_are_all_or_nothing_and_undo_exactly(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let original = document(c, &mut ids);
        let mut doc = original.clone();
        let mut history = History::new();
        let mut states = vec![doc.clone()];
        for _ in 0..1 + c.below(5) {
            let mut scratch = doc.clone();
            let mut ops = Vec::new();
            for _ in 0..1 + c.below(4) {
                let op = op(&scratch, &mut ids, c);
                scratch.apply(&op).expect("a generated operation applies");
                ops.push(op);
            }
            // A transaction ending in an impossible operation changes nothing.
            let mut bad = ops.clone();
            bad.push(Op::DeleteText { paragraph: ObjectId(u64::MAX), offset: 0, len: 1 });
            let before = doc.clone();
            prop_assert!(doc.transact("Bad", bad).is_err());
            prop_assert_eq!(&doc, &before);

            history.push(doc.transact("Edit", ops).expect("generated operations apply"));
            prop_assert_eq!(&doc, &scratch);
            states.push(doc.clone());
        }
        for state in states.iter().rev().skip(1) {
            prop_assert_eq!(history.undo(&mut doc).unwrap().map(|t| t.name.as_str()), Some("Edit"));
            prop_assert_eq!(&doc, state);
        }
        prop_assert!(history.undo(&mut doc).unwrap().is_none());
        for state in states.iter().skip(1) {
            history.redo(&mut doc).unwrap();
            prop_assert_eq!(&doc, state);
        }
        prop_assert!(history.redo(&mut doc).unwrap().is_none());
    }

    #[test]
    fn operations_survive_serialization(values in choices()) {
        let c = &mut Choices { values: &values, at: 0 };
        let mut ids = 1000;
        let doc = document(c, &mut ids);
        let op = op(&doc, &mut ids, c);
        let text = ron::to_string(&op).unwrap();
        prop_assert_eq!(ron::from_str::<Op>(&text).unwrap(), op);
    }
}

#[test]
fn a_new_document_is_one_empty_paragraph() {
    let doc = Document::new();
    assert_eq!(doc.paragraphs().len(), 1);
    assert!(doc.paragraph(doc.paragraphs()[0]).unwrap().is_empty());
}

#[test]
fn invalid_operations_are_refused_and_change_nothing() {
    let mut doc = Document::new();
    let p = doc.paragraphs()[0];
    let before = doc.clone();
    for op in [
        Op::DeleteText {
            paragraph: p,
            offset: 0,
            len: 1,
        },
        Op::InsertText {
            paragraph: p,
            offset: 1,
            text: "x".into(),
            runs: vec![Run::new(1, Props::new())],
        },
        Op::InsertText {
            paragraph: p,
            offset: 0,
            text: "xy".into(),
            runs: vec![Run::new(1, Props::new())],
        },
        Op::JoinParagraphs {
            paragraph: p,
            offset: 0,
            next: ObjectId(99),
        },
        Op::SplitParagraph {
            paragraph: p,
            offset: 0,
            new: p,
            new_props: Props::new(),
        },
        Op::SetParaProp {
            paragraph: ObjectId(99),
            key: "style".into(),
            value: None,
        },
    ] {
        assert!(doc.apply(&op).is_err(), "{op:?}");
        assert_eq!(doc, before);
    }
}

/// The tie rules rebasing documents (`doc::rebase`'s module comment), which
/// convergence alone can't tell from their mirror images.
mod ties {
    use super::*;

    fn plain(text: &str) -> Vec<Run> {
        vec![Run::new(text.chars().count() as u32, Props::new())]
    }

    fn insert(p: ObjectId, offset: u32, text: &str) -> Op {
        Op::InsertText {
            paragraph: p,
            offset,
            text: text.into(),
            runs: plain(text),
        }
    }

    /// "hello" in one paragraph.
    fn hello() -> (Document, ObjectId) {
        let mut doc = Document::new();
        let p = doc.paragraphs()[0];
        doc.apply(&insert(p, 0, "hello")).unwrap();
        (doc, p)
    }

    /// `first` then `second` rebased over it.
    fn both(doc: &Document, first: &Op, second: &Op) -> Document {
        let mut d = doc.clone();
        d.apply(first).unwrap();
        d.apply_all(&rebase(second, first, false)).unwrap();
        d
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.paragraphs()
            .iter()
            .map(|p| doc.paragraph(*p).unwrap().text.clone())
            .collect()
    }

    #[test]
    fn inserts_at_one_offset_put_the_first_text_first() {
        let (doc, p) = hello();
        let d = both(&doc, &insert(p, 2, "AB"), &insert(p, 2, "xy"));
        assert_eq!(texts(&d), ["heABxyllo"]);
    }

    #[test]
    fn an_insert_at_a_split_point_ends_the_first_paragraph() {
        let (doc, p) = hello();
        let split = Op::SplitParagraph {
            paragraph: p,
            offset: 2,
            new: ObjectId(50),
            new_props: Props::new(),
        };
        assert_eq!(
            texts(&both(&doc, &split, &insert(p, 2, "XY"))),
            ["heXY", "llo"]
        );
        assert_eq!(
            texts(&both(&doc, &insert(p, 2, "XY"), &split)),
            ["heXY", "llo"]
        );
    }

    #[test]
    fn a_deletion_leaves_text_inserted_where_it_starts_or_inside_it() {
        let (doc, p) = hello();
        let delete = Op::DeleteText {
            paragraph: p,
            offset: 1,
            len: 3,
        };
        assert_eq!(texts(&both(&doc, &insert(p, 1, "XY"), &delete)), ["hXYo"]);
        assert_eq!(texts(&both(&doc, &insert(p, 2, "XY"), &delete)), ["hXYo"]);
        assert_eq!(texts(&both(&doc, &insert(p, 4, "XY"), &delete)), ["hXYo"]);
    }

    #[test]
    fn splits_at_one_offset_put_the_first_new_paragraph_first_and_empty() {
        let (doc, p) = hello();
        let one = Op::SplitParagraph {
            paragraph: p,
            offset: 2,
            new: ObjectId(50),
            new_props: Props::new(),
        };
        let two = Op::SplitParagraph {
            paragraph: p,
            offset: 2,
            new: ObjectId(51),
            new_props: Props::new(),
        };
        let d = both(&doc, &one, &two);
        assert_eq!(d.paragraphs(), [p, ObjectId(50), ObjectId(51)]);
        assert_eq!(texts(&d), ["he", "", "llo"]);
    }

    #[test]
    fn the_later_of_two_settings_wins_where_they_overlap() {
        let (doc, p) = hello();
        let set = |offset, len, size| Op::SetCharProp {
            paragraph: p,
            offset,
            len,
            key: "size".into(),
            value: Some(Value::Int(size)),
        };
        let d = both(&doc, &set(0, 3, 10), &set(2, 3, 20));
        let sizes: Vec<_> = d
            .paragraph(p)
            .unwrap()
            .runs
            .iter()
            .map(|r| (r.len, r.props.get("size").cloned()))
            .collect();
        assert_eq!(
            sizes,
            [(2, Some(Value::Int(10))), (3, Some(Value::Int(20)))]
        );
        let para = |v: &str| Op::SetParaProp {
            paragraph: p,
            key: "style".into(),
            value: Some(Value::Text(v.into())),
        };
        let d = both(&doc, &para("Body"), &para("Quote"));
        assert_eq!(
            d.paragraph(p).unwrap().props.get("style"),
            Some(&Value::Text("Quote".into()))
        );
    }
}

mod snapshots {
    use std::sync::Arc;

    use doc::{Document, Object, ObjectId, Op, Props, Run, Snapshot, SnapshotError};

    fn two_paragraphs() -> Snapshot {
        let mut doc = Document::new();
        let first = doc.paragraphs()[0];
        doc.apply(&Op::InsertText {
            paragraph: first,
            offset: 0,
            text: "ab".to_string(),
            runs: vec![Run::new(2, Props::new())],
        })
        .unwrap();
        let new = doc.new_id();
        doc.apply(&Op::SplitParagraph {
            paragraph: first,
            offset: 1,
            new,
            new_props: Props::new(),
        })
        .unwrap();
        doc.to_snapshot()
    }

    fn paragraph_mut(s: &mut Snapshot, id: ObjectId) -> &mut doc::Paragraph {
        match &mut s.objects.iter_mut().find(|(i, _)| *i == id).unwrap().1 {
            Object::Paragraph(p) => p,
            Object::Story(_) => panic!("a paragraph"),
        }
    }

    #[test]
    fn broken_snapshots_are_refused() {
        let good = two_paragraphs();
        assert!(Document::from_snapshot(good.clone()).is_ok());
        let (root, p1, p2) = (good.root, good.objects[1].0, good.objects[2].0);

        let mut s = good.clone();
        s.objects.push(s.objects[1].clone());
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::Duplicate(p1))
        );

        let mut s = good.clone();
        s.next_id = p2.0;
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::IdTooHigh(p2))
        );

        let mut s = good.clone();
        s.root = p1;
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::RootNotAStory(p1))
        );

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story { paragraphs: vec![] });
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::EmptyStory(root))
        );

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story {
            paragraphs: vec![p1, ObjectId(99)],
        });
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::NotItsParagraph {
                story: root,
                paragraph: ObjectId(99)
            })
        );

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story {
            paragraphs: vec![p1, p1],
        });
        assert_eq!(Document::from_snapshot(s), Err(SnapshotError::Stray(p1)));

        let mut s = good.clone();
        s.objects[0].1 = Object::Story(doc::Story {
            paragraphs: vec![p1],
        });
        assert_eq!(Document::from_snapshot(s), Err(SnapshotError::Stray(p2)));

        let mut s = good.clone();
        paragraph_mut(&mut s, p1).text.push('x');
        assert_eq!(
            Document::from_snapshot(s),
            Err(SnapshotError::RunsMismatch(p1))
        );
    }

    #[test]
    fn runs_read_back_normalized() {
        let mut s = two_paragraphs();
        let p1 = s.objects[1].0;
        let p = paragraph_mut(&mut s, p1);
        p.text = "xyz".to_string();
        p.runs = vec![
            Run::new(1, Props::new()),
            Run::new(0, Props::new()),
            Run::new(2, Props::new()),
        ];
        let doc = Document::from_snapshot(s).unwrap();
        assert_eq!(
            doc.paragraph(p1).unwrap().runs,
            vec![Run {
                len: 3,
                props: Arc::new(Props::new())
            }]
        );
    }

    #[test]
    fn history_rebuilds_from_a_record_without_applying() {
        let mut history = doc::History::new();
        let t = |name: &str| doc::Transaction {
            name: name.to_string(),
            ops: vec![],
            inverse: vec![],
        };
        history.push(t("Typing"));
        history.push(t("Bold"));
        assert!(history.replay_undo());
        assert_eq!(
            (history.undo_name(), history.redo_name()),
            (Some("Typing"), Some("Bold"))
        );
        assert!(history.replay_redo());
        assert_eq!(
            (history.undo_name(), history.redo_name()),
            (Some("Bold"), None)
        );
        assert!(!history.replay_redo());
    }
}
