//! The story's tree as a reading-order list of paragraphs, each with its
//! place: how far it's indented, the marker of the list item it starts,
//! the quotes it's in, the table cell it is, and the containers around it.

use doc::{Document, Kind, Object, ObjectId, Value};

use crate::{StyleSheet, keys};

/// A container around a paragraph, for the document's structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ancestor {
    /// The container.
    pub id: ObjectId,
    /// What it is.
    pub kind: Kind,
    /// For a list, whether it's numbered.
    pub ordered: bool,
}

/// A paragraph that's a table's cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    /// The table.
    pub table: ObjectId,
    /// The row.
    pub row: ObjectId,
    /// The row's place in the table: 0 is the header.
    pub index: u32,
    /// Whether the row is the table's last.
    pub last: bool,
    /// The cell's column.
    pub column: u32,
    /// The table's columns: the most cells of any of its rows.
    pub columns: u32,
    /// Where the cell's left edge is, across from the body's start edge.
    pub x: i64,
    /// The cell's width.
    pub width: i64,
}

/// Where a paragraph sits in the story's tree, as laying it out and drawing
/// it need. Lengths are in layout units.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Place {
    /// Where its text starts, across from the body's start edge.
    pub indent: i64,
    /// How wide its text is.
    pub width: i64,
    /// The marker before its first line, if it starts a list item: a
    /// bullet, or the item's number.
    pub marker: Option<String>,
    /// Where the marker ends, across from the body's start edge: where its
    /// item's blocks start.
    pub gutter: i64,
    /// The quotes it's in, outermost first: each one, and where its bar
    /// is, across from the body's start edge.
    pub quotes: Vec<(ObjectId, i64)>,
    /// The table cell it is, if it's one.
    pub cell: Option<Cell>,
    /// The containers it's in, outermost first.
    pub path: Vec<Ancestor>,
}

impl Place {
    /// The place of a paragraph straight in the story, in a body `width`
    /// wide.
    pub fn top(width: i64) -> Place {
        Place {
            width,
            ..Place::default()
        }
    }
}

/// A paragraph of the story, in reading order, and its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The paragraph.
    pub id: ObjectId,
    /// Its place.
    pub place: Place,
}

/// The deepest nesting of containers that lays out as nesting: deeper
/// ones lay out as if they weren't there, so a file nested a million deep
/// lays out in time linear in its size.
pub const DEEPEST: usize = 32;

/// A story's paragraphs in reading order, each in its place, and the
/// containers that placed them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Flow {
    /// The paragraphs, in reading order, with their places.
    pub entries: Vec<Entry>,
    /// Every container in the story, the story first: while none of them
    /// changes, nor does the flow.
    pub containers: Vec<ObjectId>,
}

/// Every paragraph of `document`'s story, in reading order, with its
/// place, for a body `width` wide. It walks the tree without recursion,
/// so a deeply nested document can't exhaust the stack.
pub fn flow(document: &Document, sheet: &StyleSheet, width: i64) -> Flow {
    let blocks = &sheet.blocks;
    let (list_indent, quote_indent) = (blocks.list_indent.units(), blocks.quote_indent.units());
    let padding = blocks.cell_padding.units();
    let mut out = Vec::new();
    let mut containers = Vec::new();
    let mut stack = vec![(document.root(), Place::top(width))];
    while let Some((id, place)) = stack.pop() {
        let Some(object) = document.object(id) else {
            continue;
        };
        if object.is_container() {
            containers.push(id);
        }
        let block = match object {
            Object::Paragraph(_) => {
                out.push(Entry { id, place });
                continue;
            }
            Object::Story(story) => {
                stack.extend(story.blocks.iter().rev().map(|&c| (c, place.clone())));
                continue;
            }
            Object::Block(block) if place.path.len() >= DEEPEST => {
                stack.extend(block.children.iter().rev().map(|&c| (c, place.clone())));
                continue;
            }
            Object::Block(block) => block,
        };
        let mut inner = place.clone();
        let ordered = matches!(block.props.get(keys::ORDERED), Some(Value::Bool(true)));
        inner.path.push(crate::flow::Ancestor {
            id,
            kind: block.kind,
            ordered,
        });
        // What a container passes on: the marker only to its first child.
        let mut children: Vec<(ObjectId, Place)> = Vec::with_capacity(block.children.len());
        match block.kind {
            Kind::List => {
                let depth = place.path.iter().filter(|a| a.kind == Kind::List).count();
                let start = match block.props.get(keys::START) {
                    Some(Value::Int(n)) => *n,
                    _ => 1,
                };
                inner.indent = place.indent + list_indent;
                inner.gutter = inner.indent;
                inner.width = place.width - list_indent;
                for (i, &item) in block.children.iter().enumerate() {
                    let mut p = inner.clone();
                    p.marker = Some(if ordered {
                        format!("{}.", start.saturating_add(i as i64))
                    } else {
                        blocks
                            .bullets
                            .get(depth % blocks.bullets.len().max(1))
                            .cloned()
                            .unwrap_or_default()
                    });
                    children.push((item, p));
                }
            }
            Kind::Quote => {
                inner.quotes.push((id, place.indent));
                inner.indent = place.indent + quote_indent;
                inner.width = place.width - quote_indent;
                for (i, &c) in block.children.iter().enumerate() {
                    let mut p = inner.clone();
                    if i > 0 {
                        p.marker = None;
                    }
                    children.push((c, p));
                }
            }
            Kind::Table => {
                inner.marker = None;
                let rows = &block.children;
                let columns = rows
                    .iter()
                    .map(|&r| document.children(r).len())
                    .max()
                    .unwrap_or(0)
                    .max(1) as u32;
                let column_width = place.width / i64::from(columns);
                containers.extend(rows);
                for (index, &row) in rows.iter().enumerate() {
                    for (column, &cell) in document.children(row).iter().enumerate() {
                        let x = place.indent + column as i64 * column_width;
                        let mut p = inner.clone();
                        p.path.push(crate::flow::Ancestor {
                            id: row,
                            kind: Kind::Row,
                            ordered: false,
                        });
                        p.indent = x + padding;
                        p.width = (column_width - 2 * padding).max(1);
                        p.cell = Some(Cell {
                            table: id,
                            row,
                            index: index as u32,
                            last: index + 1 == rows.len(),
                            column: column as u32,
                            columns,
                            x,
                            width: column_width,
                        });
                        children.push((cell, p));
                    }
                }
            }
            // An item hands its marker on to its first block; a row out of
            // a table can't be, as rows are only ever in tables.
            Kind::Item | Kind::Row => {
                for (i, &c) in block.children.iter().enumerate() {
                    let mut p = inner.clone();
                    if i > 0 {
                        p.marker = None;
                    }
                    children.push((c, p));
                }
            }
        }
        stack.extend(children.into_iter().rev());
    }
    Flow {
        entries: out,
        containers,
    }
}
