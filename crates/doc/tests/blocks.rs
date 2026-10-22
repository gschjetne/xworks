//! The block tree (docs/ROADMAP.md, M2 stage 2): what operations on blocks
//! refuse, how snapshots of trees are checked, and the tie rules of
//! rebasing blocks (`doc::rebase`'s module comment), which convergence
//! alone can't tell from their mirror images.

use doc::{
    Block, Document, Error, Kind, Object, ObjectId, Op, Paragraph, Props, Run, SnapshotError,
    Value, conflict, rebase,
};

fn para(parent: ObjectId, text: &str) -> Object {
    let n = text.chars().count() as u32;
    Object::Paragraph(Paragraph {
        parent,
        text: text.to_string(),
        runs: if n == 0 {
            Vec::new()
        } else {
            vec![Run::new(n, Props::new())]
        },
        props: Props::new(),
    })
}

fn container(kind: Kind, parent: ObjectId, children: &[u64]) -> Object {
    Object::Block(Block {
        kind,
        parent,
        children: children.iter().map(|&c| ObjectId(c)).collect(),
        props: Props::new(),
    })
}

/// A paragraph inserted after `after` in `parent`.
fn insert_para(parent: ObjectId, after: Option<ObjectId>, id: u64, text: &str) -> Op {
    Op::InsertBlock {
        parent,
        after,
        objects: vec![(ObjectId(id), para(parent, text))],
    }
}

/// A list of one item per text, inserted after `after` in `parent`, with
/// IDs from `id` on: the list, then each item and its paragraph.
fn insert_list(parent: ObjectId, after: Option<ObjectId>, id: u64, texts: &[&str]) -> Op {
    let list = ObjectId(id);
    let items: Vec<u64> = (0..texts.len() as u64).map(|i| id + 1 + 2 * i).collect();
    let mut objects = vec![(list, container(Kind::List, parent, &items))];
    for (i, text) in texts.iter().enumerate() {
        let item = ObjectId(items[i]);
        objects.push((item, container(Kind::Item, list, &[items[i] + 1])));
        objects.push((ObjectId(items[i] + 1), para(item, text)));
    }
    Op::InsertBlock {
        parent,
        after,
        objects,
    }
}

fn texts(doc: &Document) -> Vec<String> {
    doc.paragraphs()
        .iter()
        .map(|p| doc.paragraph(*p).unwrap().text.clone())
        .collect()
}

/// "one", "two", "three" in the story, as #1, #10 and #11.
fn three() -> Document {
    let mut doc = Document::new();
    let root = doc.root();
    let first = doc.paragraphs()[0];
    doc.apply(&Op::InsertText {
        paragraph: first,
        offset: 0,
        text: "one".into(),
        runs: vec![Run::new(3, Props::new())],
    })
    .unwrap();
    doc.apply(&insert_para(root, Some(first), 10, "two"))
        .unwrap();
    doc.apply(&insert_para(root, Some(ObjectId(10)), 11, "three"))
        .unwrap();
    doc
}

/// `first` then `second` rebased over it, both made on `doc`.
fn both(doc: &Document, first: &Op, second: &Op) -> Document {
    let mut d = doc.clone();
    d.apply(first).unwrap();
    d.apply_all(&rebase(doc, second, first, false)).unwrap();
    // The other order agrees.
    let mut e = doc.clone();
    e.apply(second).unwrap();
    e.apply_all(&rebase(doc, first, second, true)).unwrap();
    assert_eq!(d, e, "both orders meet");
    d
}

#[test]
fn blocks_nest_and_read_in_order() {
    let mut doc = three();
    let root = doc.root();
    doc.apply(&insert_list(root, Some(ObjectId(10)), 20, &["a", "b"]))
        .unwrap();
    assert_eq!(texts(&doc), ["one", "two", "a", "b", "three"]);
    assert_eq!(doc.children(root).len(), 4);
    let item = ObjectId(23);
    assert_eq!(doc.parent(ObjectId(24)), Some(item));
    assert!(doc.contains(ObjectId(20), ObjectId(24)));
    assert!(!doc.contains(ObjectId(24), ObjectId(20)));
    assert_eq!(doc.previous(item), Some(ObjectId(21)));
    assert_eq!(doc.next(ObjectId(21)), Some(item));
    // Splitting a paragraph in an item keeps both halves in it.
    doc.apply(&Op::SplitParagraph {
        paragraph: ObjectId(24),
        offset: 0,
        new: ObjectId(30),
        new_props: Props::new(),
    })
    .unwrap();
    assert_eq!(doc.children(item), [ObjectId(24), ObjectId(30)]);
    assert!(doc.dump().contains("#20 list in #0 [#21, #23] {}"));
}

#[test]
fn a_deletion_undoes_with_everything_in_it() {
    let mut doc = three();
    let root = doc.root();
    doc.apply(&insert_list(root, None, 20, &["a", "b"]))
        .unwrap();
    doc.apply(&Op::SetCharProp {
        paragraph: ObjectId(24),
        offset: 0,
        len: 1,
        key: "bold".into(),
        value: Some(Value::Bool(true)),
    })
    .unwrap();
    let before = doc.clone();
    let inverse = doc
        .apply(&Op::DeleteBlock {
            block: ObjectId(20),
        })
        .unwrap();
    assert_eq!(texts(&doc), ["one", "two", "three"]);
    assert!(doc.object(ObjectId(24)).is_none());
    let Op::InsertBlock { objects, .. } = &inverse[0] else {
        panic!("an insertion");
    };
    assert_eq!(objects.len(), 5, "the list, its items and their paragraphs");
    doc.apply_all(&inverse).unwrap();
    assert_eq!(doc, before);
}

#[test]
fn moves_carry_blocks_and_undo() {
    let mut doc = three();
    let root = doc.root();
    doc.apply(&insert_list(root, None, 20, &["a"])).unwrap();
    let before = doc.clone();
    // "three" into the list's item, after its paragraph.
    let inverse = doc
        .apply(&Op::MoveBlock {
            block: ObjectId(11),
            parent: ObjectId(21),
            after: Some(ObjectId(22)),
        })
        .unwrap();
    assert_eq!(texts(&doc), ["a", "three", "one", "two"]);
    assert_eq!(doc.parent(ObjectId(11)), Some(ObjectId(21)));
    doc.apply_all(&inverse).unwrap();
    assert_eq!(doc, before);
}

#[test]
fn invalid_block_operations_are_refused_and_change_nothing() {
    let mut doc = three();
    let root = doc.root();
    doc.apply(&insert_list(root, None, 20, &["a"])).unwrap();
    let before = doc.clone();
    let cases = [
        // A paragraph straight into a list, and an item into the story.
        (
            insert_para(ObjectId(20), None, 50, "x"),
            Error::WrongKind {
                parent: ObjectId(20),
                child: ObjectId(50),
            },
        ),
        (
            Op::MoveBlock {
                block: ObjectId(21),
                parent: root,
                after: None,
            },
            Error::WrongKind {
                parent: root,
                child: ObjectId(21),
            },
        ),
        // An ID in use, and a place after something elsewhere.
        (
            insert_para(root, None, 10, "x"),
            Error::IdInUse(ObjectId(10)),
        ),
        (
            insert_para(root, Some(ObjectId(22)), 50, "x"),
            Error::NotAChild {
                parent: root,
                child: ObjectId(22),
            },
        ),
        // Objects that don't make one block.
        (
            Op::InsertBlock {
                parent: root,
                after: None,
                objects: vec![
                    (ObjectId(50), container(Kind::Quote, root, &[])),
                    (ObjectId(51), para(ObjectId(50), "loose")),
                ],
            },
            Error::Malformed(ObjectId(50)),
        ),
        (
            Op::InsertBlock {
                parent: root,
                after: None,
                objects: vec![(ObjectId(50), para(ObjectId(20), "elsewhere"))],
            },
            Error::Malformed(ObjectId(50)),
        ),
        // A block into itself, the root, and the last paragraph.
        (
            Op::MoveBlock {
                block: ObjectId(20),
                parent: ObjectId(21),
                after: None,
            },
            Error::Cycle {
                block: ObjectId(20),
                parent: ObjectId(21),
            },
        ),
        (Op::DeleteBlock { block: root }, Error::Root(root)),
        (
            Op::DeleteBlock {
                block: ObjectId(77),
            },
            Error::NoSuchBlock(ObjectId(77)),
        ),
        (
            Op::SetBlockProp {
                block: ObjectId(10),
                key: "ordered".into(),
                value: None,
            },
            Error::NoSuchBlock(ObjectId(10)),
        ),
    ];
    for (op, error) in cases {
        assert_eq!(doc.apply(&op), Err(error), "{op:?}");
        assert_eq!(doc, before);
    }
    let mut alone = Document::new();
    let only = alone.paragraphs()[0];
    assert_eq!(
        alone.apply(&Op::DeleteBlock { block: only }),
        Err(Error::LastParagraph(only))
    );
}

#[test]
fn snapshots_of_trees_are_checked() {
    let mut doc = three();
    let root = doc.root();
    doc.apply(&insert_list(root, None, 20, &["a"])).unwrap();
    let good = doc.to_snapshot();
    assert_eq!(Document::from_snapshot(good.clone()).unwrap(), doc);
    let set = |s: &mut doc::Snapshot, id: u64, object: Object| {
        s.objects.iter_mut().find(|(i, _)| i.0 == id).unwrap().1 = object;
    };
    // A list holding a paragraph.
    let mut s = good.clone();
    set(&mut s, 20, container(Kind::List, root, &[22]));
    set(&mut s, 22, para(ObjectId(20), "a"));
    set(&mut s, 21, container(Kind::Item, ObjectId(20), &[]));
    assert!(matches!(
        Document::from_snapshot(s),
        Err(SnapshotError::WrongKind { .. }) | Err(SnapshotError::Stray(_))
    ));
    // Two quotes holding each other, apart from the story.
    let mut s = good.clone();
    s.next_id = 100;
    s.objects
        .push((ObjectId(90), container(Kind::Quote, ObjectId(91), &[91])));
    s.objects
        .push((ObjectId(91), container(Kind::Quote, ObjectId(90), &[90])));
    assert!(matches!(
        Document::from_snapshot(s),
        Err(SnapshotError::Stray(_))
    ));
    // A story whose only blocks are empty containers has no paragraph.
    let mut s = doc::Snapshot {
        root,
        next_id: 3,
        objects: vec![
            (
                root,
                Object::Story(doc::Story {
                    blocks: vec![ObjectId(2)],
                }),
            ),
            (ObjectId(2), container(Kind::Quote, root, &[])),
        ],
    };
    assert_eq!(
        Document::from_snapshot(s.clone()),
        Err(SnapshotError::EmptyStory(root))
    );
    s.objects.push((ObjectId(1), para(ObjectId(2), "")));
    s.objects[1].1 = container(Kind::Quote, root, &[1]);
    assert!(Document::from_snapshot(s).is_ok());
}

mod ties {
    use super::*;

    #[test]
    fn blocks_put_in_one_place_put_the_first_first() {
        let doc = three();
        let root = doc.root();
        let d = both(
            &doc,
            &insert_para(root, Some(ObjectId(10)), 50, "A"),
            &insert_para(root, Some(ObjectId(10)), 60, "B"),
        );
        assert_eq!(texts(&d), ["one", "two", "A", "B", "three"]);
        let d = both(
            &doc,
            &Op::MoveBlock {
                block: ObjectId(1),
                parent: root,
                after: Some(ObjectId(11)),
            },
            &insert_para(root, Some(ObjectId(11)), 60, "B"),
        );
        assert_eq!(texts(&d), ["two", "three", "one", "B"]);
    }

    #[test]
    fn a_place_after_a_deleted_or_moved_block_is_after_the_one_before() {
        let doc = three();
        let root = doc.root();
        let after_two = insert_para(root, Some(ObjectId(10)), 50, "X");
        let d = both(
            &doc,
            &Op::DeleteBlock {
                block: ObjectId(10),
            },
            &after_two,
        );
        assert_eq!(texts(&d), ["one", "X", "three"]);
        let d = both(
            &doc,
            &Op::MoveBlock {
                block: ObjectId(10),
                parent: root,
                after: Some(ObjectId(11)),
            },
            &after_two,
        );
        assert_eq!(texts(&d), ["one", "X", "three", "two"]);
    }

    #[test]
    fn a_place_after_a_split_paragraph_is_after_its_second_half() {
        let doc = three();
        let root = doc.root();
        let split = Op::SplitParagraph {
            paragraph: ObjectId(10),
            offset: 1,
            new: ObjectId(40),
            new_props: Props::new(),
        };
        let d = both(
            &doc,
            &split,
            &insert_para(root, Some(ObjectId(10)), 50, "X"),
        );
        assert_eq!(texts(&d), ["one", "t", "wo", "X", "three"]);
    }

    #[test]
    fn a_deleted_block_takes_the_changes_inside_it() {
        let mut doc = three();
        let root = doc.root();
        doc.apply(&insert_list(root, None, 20, &["a", "b"]))
            .unwrap();
        let delete = Op::DeleteBlock {
            block: ObjectId(20),
        };
        let typing = Op::InsertText {
            paragraph: ObjectId(22),
            offset: 1,
            text: "!".into(),
            runs: vec![Run::new(1, Props::new())],
        };
        assert_eq!(
            texts(&both(&doc, &delete, &typing)),
            ["one", "two", "three"]
        );
        assert_eq!(
            texts(&both(&doc, &typing, &delete)),
            ["one", "two", "three"]
        );
        // A block moved into it stays where it was; one moved out of it
        // goes with it all the same.
        let into = Op::MoveBlock {
            block: ObjectId(11),
            parent: ObjectId(21),
            after: None,
        };
        assert_eq!(texts(&both(&doc, &delete, &into)), ["one", "two", "three"]);
        assert_eq!(texts(&both(&doc, &into, &delete)), ["one", "two", "three"]);
        let out = Op::MoveBlock {
            block: ObjectId(22),
            parent: root,
            after: None,
        };
        assert_eq!(texts(&both(&doc, &delete, &out)), ["one", "two", "three"]);
        assert_eq!(texts(&both(&doc, &out, &delete)), ["one", "two", "three"]);
    }

    #[test]
    fn a_paragraph_split_or_joined_while_it_goes() {
        let doc = three();
        let split = Op::SplitParagraph {
            paragraph: ObjectId(10),
            offset: 1,
            new: ObjectId(40),
            new_props: Props::new(),
        };
        let delete = Op::DeleteBlock {
            block: ObjectId(10),
        };
        assert_eq!(texts(&both(&doc, &split, &delete)), ["one", "three"]);
        assert_eq!(texts(&both(&doc, &delete, &split)), ["one", "three"]);
        let join = Op::JoinParagraphs {
            paragraph: ObjectId(1),
            offset: 3,
            next: ObjectId(10),
        };
        let delete_next = Op::DeleteBlock {
            block: ObjectId(10),
        };
        assert_eq!(texts(&both(&doc, &join, &delete_next)), ["one", "three"]);
        let delete_first = Op::DeleteBlock { block: ObjectId(1) };
        assert_eq!(texts(&both(&doc, &join, &delete_first)), ["two", "three"]);
        // A block put between two paragraphs joining goes after them.
        let between = insert_para(doc.root(), Some(ObjectId(1)), 50, "X");
        assert_eq!(
            texts(&both(&doc, &join, &between)),
            ["onetwo", "X", "three"]
        );
    }

    #[test]
    fn moves_that_would_nest_a_block_in_itself_keep_the_first() {
        let mut doc = Document::new();
        let root = doc.root();
        doc.apply(&Op::InsertBlock {
            parent: root,
            after: None,
            objects: vec![
                (ObjectId(10), container(Kind::Quote, root, &[11])),
                (ObjectId(11), para(ObjectId(10), "x")),
            ],
        })
        .unwrap();
        doc.apply(&Op::InsertBlock {
            parent: root,
            after: Some(ObjectId(10)),
            objects: vec![
                (ObjectId(20), container(Kind::Quote, root, &[21])),
                (ObjectId(21), para(ObjectId(20), "y")),
            ],
        })
        .unwrap();
        let ten_in_twenty = Op::MoveBlock {
            block: ObjectId(10),
            parent: ObjectId(20),
            after: None,
        };
        let twenty_in_ten = Op::MoveBlock {
            block: ObjectId(20),
            parent: ObjectId(10),
            after: None,
        };
        // Each alone is fine; together they'd be a ring, which only the
        // tree shows.
        assert!(conflict(&doc, &ten_in_twenty, &twenty_in_ten));
        let d = both(&doc, &ten_in_twenty, &twenty_in_ten);
        assert_eq!(d.parent(ObjectId(10)), Some(ObjectId(20)));
        assert_eq!(d.parent(ObjectId(20)), Some(root));
        let d = both(&doc, &twenty_in_ten, &ten_in_twenty);
        assert_eq!(d.parent(ObjectId(20)), Some(ObjectId(10)));
        // The same block moved twice: the first move wins.
        let ten_last = Op::MoveBlock {
            block: ObjectId(10),
            parent: root,
            after: Some(ObjectId(20)),
        };
        let d = both(&doc, &ten_in_twenty, &ten_last);
        assert_eq!(d.parent(ObjectId(10)), Some(ObjectId(20)));
    }

    #[test]
    fn deletions_that_would_leave_no_paragraph_keep_the_first() {
        let mut doc = Document::new();
        let root = doc.root();
        let first = doc.paragraphs()[0];
        doc.apply(&insert_para(root, Some(first), 10, "two"))
            .unwrap();
        let one = Op::DeleteBlock { block: first };
        let two = Op::DeleteBlock {
            block: ObjectId(10),
        };
        assert!(conflict(&doc, &one, &two));
        assert_eq!(texts(&both(&doc, &one, &two)), ["two"]);
        assert_eq!(texts(&both(&doc, &two, &one)), [""]);
    }

    #[test]
    fn the_later_of_two_settings_of_a_block_wins() {
        let mut doc = three();
        doc.apply(&insert_list(doc.root(), None, 20, &["a"]))
            .unwrap();
        let set = |v| Op::SetBlockProp {
            block: ObjectId(20),
            key: "ordered".into(),
            value: Some(Value::Bool(v)),
        };
        let d = both(&doc, &set(true), &set(false));
        assert_eq!(
            d.block(ObjectId(20)).unwrap().props.get("ordered"),
            Some(&Value::Bool(false))
        );
    }
}
