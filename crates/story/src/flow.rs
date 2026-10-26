//! The story's tree as a reading-order list of paragraphs, each with its
//! place: how far it's indented, the marker of the list item it starts or
//! the number of the numbered heading it is, the quotes it's in and
//! whether it starts or ends one, the table cell it is, the containers
//! around it, and the rules across the text that come before it.

use doc::{Document, Kind, Object, ObjectId, Value};

use crate::{Numbering, StyleSheet, keys};

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

/// A rule across the text, Markdown's thematic break, in its place: drawn
/// across its column, in the quotes it's in, before the paragraph it's
/// placed with, or after the story's last paragraph if none follows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rule {
    /// The rule.
    pub id: ObjectId,
    /// Where it starts, across from the body's start edge.
    pub indent: i64,
    /// How wide it is.
    pub width: i64,
    /// The quotes it's in, outermost first, as [`Place::quotes`] has them.
    pub quotes: Vec<(ObjectId, i64)>,
    /// Whether a quote starts with it, as [`Place::quote_above`] says.
    pub quote_above: bool,
    /// Whether a quote ends with it, as [`Place::quote_below`] says.
    pub quote_below: bool,
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
    /// bullet, or the item's number; or a numbered heading's number.
    pub marker: Option<String>,
    /// The room between its marker and its text.
    pub gap: i64,
    /// The box before its first line, if it starts a task's item, in place
    /// of a bullet: whether it's checked.
    pub task: Option<bool>,
    /// Where the marker ends, across from the body's start edge: where its
    /// item's blocks start.
    pub gutter: i64,
    /// The quotes it's in, outermost first: each one, and where its bar
    /// is, across from the body's start edge.
    pub quotes: Vec<(ObjectId, i64)>,
    /// Whether a quote starts with it, in reading order: one it's in that
    /// what comes before it, a paragraph or a rule, isn't. The quote's
    /// space goes above it, once, however many start there.
    pub quote_above: bool,
    /// Whether a quote ends with it: one it's in that what comes after it
    /// isn't. The quote's space goes below it, once.
    pub quote_below: bool,
    /// The table cell it is, if it's one.
    pub cell: Option<Cell>,
    /// The containers it's in, outermost first.
    pub path: Vec<Ancestor>,
    /// The rules across the text right before it in reading order, which
    /// it's placed with, in order.
    pub rules: Vec<Rule>,
    /// The rules after it, if it's the story's last paragraph and rules
    /// end the story.
    pub rules_after: Vec<Rule>,
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
    let quote_indent = blocks.quote_indent;
    let padding = blocks.cell_padding;
    // How a numbered heading's number reads: as the list definition with
    // the role "headings" writes numbers, or as numbered lists do.
    let headings = sheet
        .lists
        .iter()
        .find(|l| l.role.as_deref() == Some("headings"))
        .unwrap_or_else(|| sheet.list(true, None));
    // How many numbered headings of each level there have been since the
    // last of a level above it, counted from the number before the one
    // its level of the definition starts at.
    let first = |k: usize| headings.level(k).start.saturating_sub(1);
    let mut counters: [i64; 6] = std::array::from_fn(first);
    let mut out: Vec<Entry> = Vec::new();
    let mut containers = Vec::new();
    // The rules met since the last paragraph, which go with the next.
    let mut rules: Vec<Rule> = Vec::new();
    let mut stack = vec![(document.root(), Place::top(width))];
    while let Some((id, place)) = stack.pop() {
        let Some(object) = document.object(id) else {
            continue;
        };
        if object.is_container() {
            containers.push(id);
        }
        let block = match object {
            Object::Paragraph(p) => {
                let mut place = place;
                place.rules = std::mem::take(&mut rules);
                let style = sheet.style_of(p);
                if let Some(level) = style.heading.filter(|l| (1..=6).contains(l)) {
                    let level = usize::from(level);
                    if style.numbered {
                        counters[level - 1] = counters[level - 1].saturating_add(1);
                        for (k, counter) in counters.iter_mut().enumerate().skip(level) {
                            *counter = first(k);
                        }
                        // Each level's count in its level's numbering,
                        // read as the heading's level reads numbers.
                        let number: Vec<String> = counters[..level]
                            .iter()
                            .enumerate()
                            .map(|(k, n)| headings.level(k).numbering(true).number(*n))
                            .collect();
                        let own = headings.level(level - 1);
                        if place.marker.is_none() {
                            place.marker = Some(own.reads(&number.join(".")));
                            place.gutter = place.indent;
                            place.gap = own.gap;
                        }
                    }
                }
                out.push(Entry { id, place });
                continue;
            }
            Object::Block(block) if block.kind == Kind::Rule => {
                rules.push(Rule {
                    id,
                    indent: place.indent,
                    width: place.width,
                    quotes: place.quotes.clone(),
                    quote_above: false,
                    quote_below: false,
                });
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
            // The stylesheet isn't in the story.
            Object::Sheet(_) | Object::Style(_) => continue,
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
                let own = match block.props.get(keys::LIST) {
                    Some(Value::Ref(id)) => Some(*id),
                    _ => None,
                };
                // The level of its definition its depth takes: its marker,
                // where it starts counting, and where it sits.
                let level = sheet.list(ordered, own).level(depth);
                let start = match block.props.get(keys::START) {
                    Some(Value::Int(n)) => *n,
                    _ => level.start,
                };
                let bullet = if level.bullet.is_empty() {
                    &sheet.list(false, None).level(depth).bullet
                } else {
                    &level.bullet
                };
                let numbering = level.numbering(ordered);
                inner.indent = place.indent + level.indent;
                inner.gutter = inner.indent;
                inner.gap = level.gap;
                inner.width = place.width - level.indent;
                for (i, &item) in block.children.iter().enumerate() {
                    let mut p = inner.clone();
                    // A task's box takes a bullet's place, but not a
                    // number's.
                    let task = match document.block(item).and_then(|b| b.props.get(keys::TASK)) {
                        Some(Value::Bool(done)) => Some(*done),
                        _ => None,
                    };
                    p.task = task;
                    p.marker = if numbering != Numbering::Bullet {
                        Some(level.marker(ordered, start.saturating_add(i as i64)))
                    } else if task.is_some() {
                        None
                    } else {
                        Some(bullet.clone())
                    };
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
                        p.task = None;
                    }
                    children.push((c, p));
                }
            }
            Kind::Table => {
                inner.marker = None;
                inner.task = None;
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
            // a table can't be, as rows are only ever in tables; and a rule
            // holds nothing.
            Kind::Item | Kind::Row | Kind::Rule => {
                for (i, &c) in block.children.iter().enumerate() {
                    let mut p = inner.clone();
                    if i > 0 {
                        p.marker = None;
                        p.task = None;
                    }
                    children.push((c, p));
                }
            }
        }
        stack.extend(children.into_iter().rev());
    }
    if let Some(last) = out.last_mut() {
        last.place.rules_after = rules;
    }
    quote_edges(&mut out);
    Flow {
        entries: out,
        containers,
    }
}

/// Where in `entries`, in reading order, a paragraph or a rule is: rule
/// `r` before entry `k`, entry `k` itself, or rule `r` after the last.
#[derive(Clone, Copy)]
enum At {
    Rule(usize, usize),
    Paragraph(usize),
    After(usize),
}

/// Marks where quotes start and end among `entries` and their rules, in
/// reading order: each paragraph or rule in a quote that what comes
/// before it isn't in starts it, and each in one that what comes after it
/// isn't in ends it.
fn quote_edges(entries: &mut [Entry]) {
    let mut order: Vec<At> = Vec::with_capacity(entries.len());
    for (k, e) in entries.iter().enumerate() {
        order.extend((0..e.place.rules.len()).map(|r| At::Rule(k, r)));
        order.push(At::Paragraph(k));
    }
    if let Some(last) = entries.last() {
        order.extend((0..last.place.rules_after.len()).map(At::After));
    }
    let quotes = |entries: &[Entry], at: At| -> Vec<ObjectId> {
        let list = match at {
            At::Rule(k, r) => &entries[k].place.rules[r].quotes,
            At::Paragraph(k) => &entries[k].place.quotes,
            At::After(r) => &entries[entries.len() - 1].place.rules_after[r].quotes,
        };
        list.iter().map(|q| q.0).collect()
    };
    // Whether `a` is in a quote `b` isn't in.
    let outside = |a: &[ObjectId], b: &[ObjectId]| a.iter().any(|q| !b.contains(q));
    let mut before: Vec<ObjectId> = Vec::new();
    let mut here = order
        .first()
        .map_or_else(Vec::new, |&at| quotes(entries, at));
    for i in 0..order.len() {
        let after = order
            .get(i + 1)
            .map_or_else(Vec::new, |&at| quotes(entries, at));
        let (above, below) = (outside(&here, &before), outside(&here, &after));
        match order[i] {
            At::Rule(k, r) => {
                let rule = &mut entries[k].place.rules[r];
                (rule.quote_above, rule.quote_below) = (above, below);
            }
            At::Paragraph(k) => {
                let place = &mut entries[k].place;
                (place.quote_above, place.quote_below) = (above, below);
            }
            At::After(r) => {
                let last = entries.len() - 1;
                let rule = &mut entries[last].place.rules_after[r];
                (rule.quote_above, rule.quote_below) = (above, below);
            }
        }
        before = std::mem::replace(&mut here, after);
    }
}
