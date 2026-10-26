//! Schema 2's documents made schema 3's: each given the stylesheet it was
//! laid out with, Neutral as it was at version 1 and a Table Header that
//! looks as header rows did, kept frozen in `spec/format/upgrade-2.ron`,
//! and its paragraphs' styles named by ID rather than by name, its tables'
//! header rows' paragraphs in Table Header, in every snapshot and every
//! log entry, so its named versions and its history still apply.
//!
//! Schema 3 only added variants after schema 2's, so schema 2's bytes read
//! as schema 3's types; what changes is what they say.

use std::collections::{HashMap, HashSet};

use doc::{Kind, Object, ObjectId, Op, Props, Snapshot, Value};

use crate::Error;

/// The stylesheet schema 2's documents were laid out with: its objects,
/// the sheet first at ID 0, then its styles and parts.
const SHEET: &str = include_str!("../../../spec/format/upgrade-2.ron");

/// The key a paragraph names its style under.
const STYLE: &str = "style";

/// The role of the frozen sheet's style for a table's header row.
const TABLE_HEADER: &str = "table-header";

/// Every paragraph that's ever a cell of a table's header row, its first
/// row, in `snapshots` or made by `ops`: in a table there or in one an
/// operation inserts, or in a row inserted first into a table either has.
/// Schema 2's tables keep their shape, rows only ever added after a row
/// and cells never moved, so such a paragraph is in a header row whenever
/// it's in the document, and naming its style pointwise, wherever it's
/// set, gives every snapshot what replaying the log does.
pub(crate) fn header_cells<'a>(
    snapshots: impl IntoIterator<Item = &'a Snapshot>,
    ops: impl IntoIterator<Item = &'a Op> + Clone,
) -> HashSet<ObjectId> {
    let mut cells = HashSet::new();
    let mut tables: HashSet<ObjectId> = HashSet::new();
    // The header row's cells among `objects`, a table's or a row's.
    let mut add = |objects: &[(ObjectId, Object)], first_rows: &[ObjectId]| {
        for (id, object) in objects {
            if let Object::Paragraph(p) = object
                && first_rows.contains(&p.parent)
            {
                cells.insert(*id);
            }
        }
    };
    let first_rows = |objects: &[(ObjectId, Object)]| -> Vec<ObjectId> {
        objects
            .iter()
            .filter_map(|(_, o)| match o {
                Object::Block(b) if b.kind == Kind::Table => b.children.first().copied(),
                _ => None,
            })
            .collect()
    };
    for snapshot in snapshots {
        let rows = first_rows(&snapshot.objects);
        add(&snapshot.objects, &rows);
        tables.extend(snapshot.objects.iter().filter_map(|(id, o)| match o {
            Object::Block(b) if b.kind == Kind::Table => Some(*id),
            _ => None,
        }));
    }
    for op in ops.clone() {
        if let Op::InsertBlock { objects, .. } = op {
            let rows = first_rows(objects);
            add(objects, &rows);
            tables.extend(objects.iter().filter_map(|(id, o)| match o {
                Object::Block(b) if b.kind == Kind::Table => Some(*id),
                _ => None,
            }));
        }
    }
    for op in ops {
        if let Op::InsertBlock {
            parent,
            after: None,
            objects,
        } = op
            && tables.contains(parent)
            && let Some((row, Object::Block(b))) = objects.first()
            && b.kind == Kind::Row
        {
            add(objects, &[*row]);
        }
    }
    cells
}

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
/// above every ID the file uses, its paragraph styles by name, and the
/// paragraphs of its tables' header rows.
pub(crate) struct Upgrade {
    objects: Vec<(ObjectId, Object)>,
    names: HashMap<String, ObjectId>,
    end: u64,
    /// The paragraphs of tables' header rows ([`header_cells`]), the style
    /// they take, and the default style, which they laid out in, bold.
    headers: HashSet<ObjectId>,
    table_header: Option<ObjectId>,
    body: Option<ObjectId>,
}

impl Upgrade {
    /// The upgrade of a file whose objects all have IDs below `top`, whose
    /// tables' header rows' paragraphs are `headers`.
    pub(crate) fn new(top: u64, headers: HashSet<ObjectId>) -> Result<Upgrade, Error> {
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
        let role = |id: &ObjectId, role: &str| match objects.iter().find(|(o, _)| o == id) {
            Some((_, Object::Style(s))) => {
                s.kind == doc::StyleKind::Paragraph
                    && s.props.get("role") == Some(&Value::Text(role.to_string()))
            }
            _ => false,
        };
        let table_header = objects
            .iter()
            .map(|(id, _)| *id)
            .find(|id| role(id, TABLE_HEADER));
        let body = objects
            .iter()
            .map(|(id, _)| *id)
            .find(|id| role(id, "body"));
        Ok(Upgrade {
            objects,
            names,
            end,
            headers,
            table_header,
            body,
        })
    }

    /// The style a header row's paragraph takes for `value`: Table Header
    /// for none, or the default style, which it laid out in, bold by its
    /// place; any other as it was.
    fn header_style(&self, value: Option<Value>) -> Option<Value> {
        match (&value, self.table_header) {
            (None, Some(header)) => Some(Value::Ref(header)),
            (Some(Value::Ref(id)), Some(header)) if Some(*id) == self.body => {
                Some(Value::Ref(header))
            }
            _ => value,
        }
    }

    /// Paragraph `id`'s properties with its style named by ID, and in a
    /// header row, as [`Upgrade::header_style`] says.
    fn paragraph_props(&self, id: ObjectId, props: &mut Props) {
        self.props(props);
        if self.headers.contains(&id)
            && let Some(style) = self.header_style(props.remove(STYLE))
        {
            props.insert(STYLE.to_string(), style);
        }
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
        for (id, object) in &mut snapshot.objects {
            if let Object::Paragraph(p) = object {
                self.paragraph_props(*id, &mut p.props);
            }
        }
        snapshot.objects.extend(self.objects.iter().cloned());
        snapshot.objects.sort_by_key(|(id, _)| *id);
        snapshot.next_id = snapshot.next_id.max(self.end);
    }

    /// `op`, with the styles it names by ID.
    pub(crate) fn op(&self, op: &mut Op) {
        match op {
            Op::SetParaProp {
                paragraph,
                key,
                value,
            } if key == STYLE => {
                *value = value.as_ref().and_then(|v| self.style(v));
                if self.headers.contains(paragraph) {
                    *value = self.header_style(value.take());
                }
            }
            Op::SplitParagraph { new, new_props, .. } => self.paragraph_props(*new, new_props),
            Op::InsertBlock { objects, .. } => {
                for (id, object) in objects {
                    if let Object::Paragraph(p) = object {
                        self.paragraph_props(*id, &mut p.props);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doc::{Block, Paragraph};

    fn block(kind: Kind, parent: u64, children: &[u64]) -> Object {
        Object::Block(Block {
            kind,
            parent: ObjectId(parent),
            children: children.iter().map(|&c| ObjectId(c)).collect(),
            props: Props::new(),
        })
    }

    fn cell(row: u64) -> Object {
        Object::Paragraph(Paragraph {
            parent: ObjectId(row),
            text: String::new(),
            runs: Vec::new(),
            props: Props::new(),
        })
    }

    /// A header row's cells are found in a snapshot's tables, in tables
    /// operations insert, and in a row inserted first into a known table.
    #[test]
    fn header_cells_are_found_wherever_a_header_row_is_made() {
        let id = ObjectId;
        let snapshot = Snapshot {
            root: id(0),
            next_id: 10,
            objects: vec![
                (id(1), block(Kind::Table, 0, &[2, 4])),
                (id(2), block(Kind::Row, 1, &[3])),
                (id(3), cell(2)),
                (id(4), block(Kind::Row, 1, &[5])),
                (id(5), cell(4)),
            ],
        };
        let ops = [
            Op::InsertBlock {
                parent: id(0),
                after: None,
                objects: vec![
                    (id(10), block(Kind::Table, 0, &[11, 13])),
                    (id(11), block(Kind::Row, 10, &[12])),
                    (id(12), cell(11)),
                    (id(13), block(Kind::Row, 10, &[14])),
                    (id(14), cell(13)),
                ],
            },
            Op::InsertBlock {
                parent: id(1),
                after: None,
                objects: vec![(id(20), block(Kind::Row, 1, &[21])), (id(21), cell(20))],
            },
            Op::InsertBlock {
                parent: id(1),
                after: Some(id(4)),
                objects: vec![(id(30), block(Kind::Row, 1, &[31])), (id(31), cell(30))],
            },
        ];
        let mut found: Vec<u64> = header_cells([&snapshot], ops.iter())
            .into_iter()
            .map(|c| c.0)
            .collect();
        found.sort_unstable();
        assert_eq!(found, [3, 12, 21]);
    }
}
