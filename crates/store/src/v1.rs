//! Schema 1's types, frozen as M1 wrote them, so its snapshots and log
//! entries decode whatever `doc` becomes, and turn into today's.
//!
//! postcard writes structs by their fields' order and enums by their
//! variants' positions; these keep M1's.

use std::collections::BTreeMap;
use std::sync::Arc;

use doc::ObjectId;
use serde::Deserialize;

/// M1's property values.
#[derive(Deserialize)]
pub(crate) enum Value {
    Bool(bool),
    Int(i64),
    Text(String),
}

type Props = BTreeMap<String, Value>;

/// M1's run of character properties.
#[derive(Deserialize)]
pub(crate) struct Run {
    len: u32,
    props: Props,
}

/// M1's operations.
#[derive(Deserialize)]
pub(crate) enum Op {
    InsertText {
        paragraph: ObjectId,
        offset: u32,
        text: String,
        runs: Vec<Run>,
    },
    DeleteText {
        paragraph: ObjectId,
        offset: u32,
        len: u32,
    },
    SplitParagraph {
        paragraph: ObjectId,
        offset: u32,
        new: ObjectId,
        new_props: Props,
    },
    JoinParagraphs {
        paragraph: ObjectId,
        offset: u32,
        next: ObjectId,
    },
    SetCharProp {
        paragraph: ObjectId,
        offset: u32,
        len: u32,
        key: String,
        value: Option<Value>,
    },
    SetParaProp {
        paragraph: ObjectId,
        key: String,
        value: Option<Value>,
    },
}

/// M1's story: a list of paragraphs.
#[derive(Deserialize)]
pub(crate) struct Story {
    paragraphs: Vec<ObjectId>,
}

/// M1's paragraph, which names its story.
#[derive(Deserialize)]
pub(crate) struct Paragraph {
    story: ObjectId,
    text: String,
    runs: Vec<Run>,
    props: Props,
}

/// M1's objects.
#[derive(Deserialize)]
pub(crate) enum Object {
    Story(Story),
    Paragraph(Paragraph),
}

/// M1's snapshot of a document.
#[derive(Deserialize)]
pub(crate) struct Snapshot {
    root: ObjectId,
    next_id: u64,
    objects: Vec<(ObjectId, Object)>,
}

impl From<Value> for doc::Value {
    fn from(value: Value) -> doc::Value {
        match value {
            Value::Bool(b) => doc::Value::Bool(b),
            Value::Int(i) => doc::Value::Int(i),
            Value::Text(t) => doc::Value::Text(t),
        }
    }
}

fn props(props: Props) -> doc::Props {
    props.into_iter().map(|(k, v)| (k, v.into())).collect()
}

fn runs(runs: Vec<Run>) -> Vec<doc::Run> {
    runs.into_iter()
        .map(|r| doc::Run {
            len: r.len,
            props: Arc::new(props(r.props)),
        })
        .collect()
}

impl From<Op> for doc::Op {
    fn from(op: Op) -> doc::Op {
        match op {
            Op::InsertText {
                paragraph,
                offset,
                text,
                runs: r,
            } => doc::Op::InsertText {
                paragraph,
                offset,
                text,
                runs: runs(r),
            },
            Op::DeleteText {
                paragraph,
                offset,
                len,
            } => doc::Op::DeleteText {
                paragraph,
                offset,
                len,
            },
            Op::SplitParagraph {
                paragraph,
                offset,
                new,
                new_props,
            } => doc::Op::SplitParagraph {
                paragraph,
                offset,
                new,
                new_props: props(new_props),
            },
            Op::JoinParagraphs {
                paragraph,
                offset,
                next,
            } => doc::Op::JoinParagraphs {
                paragraph,
                offset,
                next,
            },
            Op::SetCharProp {
                paragraph,
                offset,
                len,
                key,
                value,
            } => doc::Op::SetCharProp {
                paragraph,
                offset,
                len,
                key,
                value: value.map(Into::into),
            },
            Op::SetParaProp {
                paragraph,
                key,
                value,
            } => doc::Op::SetParaProp {
                paragraph,
                key,
                value: value.map(Into::into),
            },
        }
    }
}

impl From<Snapshot> for doc::Snapshot {
    fn from(snapshot: Snapshot) -> doc::Snapshot {
        doc::Snapshot {
            root: snapshot.root,
            next_id: snapshot.next_id,
            objects: snapshot
                .objects
                .into_iter()
                .map(|(id, object)| {
                    let object = match object {
                        Object::Story(s) => doc::Object::Story(doc::Story {
                            blocks: s.paragraphs,
                        }),
                        Object::Paragraph(p) => doc::Object::Paragraph(doc::Paragraph {
                            parent: p.story,
                            text: p.text,
                            runs: runs(p.runs),
                            props: props(p.props),
                        }),
                    };
                    (id, object)
                })
                .collect(),
        }
    }
}
