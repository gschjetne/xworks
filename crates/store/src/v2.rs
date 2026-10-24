//! Schema 2's documents made schema 3's: each given the stylesheet it was
//! laid out with, Neutral as it was at version 1, kept frozen in
//! `spec/format/upgrade-2.ron`, and its paragraphs' styles named by ID
//! rather than by name, in every snapshot and every log entry, so its
//! named versions and its history still apply.
//!
//! Schema 3 only added variants after schema 2's, so schema 2's bytes read
//! as schema 3's types; what changes is what they say.

use std::collections::HashMap;

use doc::{Object, ObjectId, Op, Props, Snapshot, Value};

use crate::Error;

/// The stylesheet schema 2's documents were laid out with: its objects,
/// the sheet first at ID 0, then its styles and parts.
const SHEET: &str = include_str!("../../../spec/format/upgrade-2.ron");

/// The key a paragraph names its style under.
const STYLE: &str = "style";

/// The frozen stylesheet's objects, at IDs from 0.
pub(crate) fn frozen() -> Result<Vec<(ObjectId, Object)>, Error> {
    ron::from_str(SHEET).map_err(|e| Error::Damaged(format!("the frozen stylesheet: {e}")))
}

/// The IDs `op` makes objects with.
pub(crate) fn created(op: &Op) -> Vec<u64> {
    match op {
        Op::SplitParagraph { new, .. } => vec![new.0],
        Op::InsertBlock { objects, .. } => objects.iter().map(|(id, _)| id.0).collect(),
        Op::InsertStyle { id, .. } => vec![id.0],
        _ => Vec::new(),
    }
}

/// `id` moved up by `by`.
fn moved(id: ObjectId, by: u64) -> ObjectId {
    ObjectId(id.0 + by)
}

/// `object`, a frozen stylesheet's, with every ID in it moved up by `by`.
fn move_object(object: &Object, by: u64) -> Object {
    let props = |props: &Props| -> Props {
        props
            .iter()
            .map(|(k, v)| match v {
                Value::Ref(id) => (k.clone(), Value::Ref(moved(*id, by))),
                other => (k.clone(), other.clone()),
            })
            .collect()
    };
    match object {
        Object::Sheet(s) => Object::Sheet(doc::Sheet {
            styles: s.styles.iter().map(|&id| moved(id, by)).collect(),
            props: props(&s.props),
        }),
        Object::Style(s) => Object::Style(doc::Style {
            sheet: moved(s.sheet, by),
            kind: s.kind,
            base: s.base.map(|b| moved(b, by)),
            props: props(&s.props),
        }),
        other => other.clone(),
    }
}

/// The upgrade of one file: the stylesheet every snapshot gains, at IDs
/// above every ID the file uses, and its paragraph styles by name.
pub(crate) struct Upgrade {
    objects: Vec<(ObjectId, Object)>,
    names: HashMap<String, ObjectId>,
    end: u64,
}

impl Upgrade {
    /// The upgrade of a file whose objects all have IDs below `top`.
    pub(crate) fn new(top: u64) -> Result<Upgrade, Error> {
        let frozen = frozen()?;
        let objects: Vec<(ObjectId, Object)> = frozen
            .iter()
            .map(|(id, o)| (moved(*id, top), move_object(o, top)))
            .collect();
        let names = objects
            .iter()
            .filter_map(|(id, o)| match o {
                Object::Style(s) if s.kind == doc::StyleKind::Paragraph => {
                    match s.props.get("name") {
                        Some(Value::Text(name)) => Some((name.clone(), *id)),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect();
        let end = objects.iter().map(|(id, _)| id.0 + 1).max().unwrap_or(top);
        Ok(Upgrade {
            objects,
            names,
            end,
        })
    }

    /// The style a paragraph names `value` as, by ID: none for a name the
    /// sheet hasn't, which laid out as the default style.
    fn style(&self, value: &Value) -> Option<Value> {
        match value {
            Value::Text(name) => self.names.get(name).map(|&id| Value::Ref(id)),
            other => Some(other.clone()),
        }
    }

    /// A paragraph's properties with its style named by ID.
    fn props(&self, props: &mut Props) {
        if let Some(value) = props.remove(STYLE)
            && let Some(id) = self.style(&value)
        {
            props.insert(STYLE.to_string(), id);
        }
    }

    /// `snapshot`, with the stylesheet, and its paragraphs' styles by ID.
    pub(crate) fn snapshot(&self, snapshot: &mut Snapshot) {
        for (_, object) in &mut snapshot.objects {
            if let Object::Paragraph(p) = object {
                self.props(&mut p.props);
            }
        }
        snapshot.objects.extend(self.objects.iter().cloned());
        snapshot.objects.sort_by_key(|(id, _)| *id);
        snapshot.next_id = snapshot.next_id.max(self.end);
    }

    /// `op`, with the styles it names by ID.
    pub(crate) fn op(&self, op: &mut Op) {
        match op {
            Op::SetParaProp { key, value, .. } if key == STYLE => {
                *value = value.as_ref().and_then(|v| self.style(v));
            }
            Op::SplitParagraph { new_props, .. } => self.props(new_props),
            Op::InsertBlock { objects, .. } => {
                for (_, object) in objects {
                    if let Object::Paragraph(p) = object {
                        self.props(&mut p.props);
                    }
                }
            }
            _ => {}
        }
    }
}
