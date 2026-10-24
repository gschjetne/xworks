//! A story as Markdown holds it: its blocks in reading order, each with
//! its depth, and only what Markdown can say of it. Two stories Markdown
//! writes alike have the same shape, so what's written can be checked by
//! reading it back.

use doc::{Document, Kind, Object, ObjectId, Paragraph, Props, Value};
use story::{OBJECT, StyleSheet, keys};

/// A block, as Markdown holds it, at a depth in the story's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// How deep it is: 0 for the story's own blocks.
    pub depth: usize,
    /// What it is.
    pub form: Form,
}

/// What a block is, as Markdown holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Form {
    /// A paragraph, heading, code block, Markdown kept as it was, or a
    /// table's cell.
    Text {
        /// "", "h1" to "h6", "code", or "source" for Markdown kept as it
        /// was written.
        kind: String,
        /// A code block's info string, or what kept Markdown is; a cell's
        /// alignment.
        detail: String,
        /// Its text.
        text: String,
        /// Its runs: each one's length and the properties Markdown holds,
        /// in order of their keys.
        runs: Vec<(u32, Vec<(String, Value)>)>,
    },
    /// A list: whether it's numbered, and from what.
    List(Option<i64>),
    /// A list's item: whether it's a task, and done.
    Item(Option<bool>),
    /// A quote.
    Quote,
    /// A table.
    Table,
    /// A table's row.
    Row,
    /// A rule across the text: a thematic break.
    Rule,
}

/// The keys of a run's properties Markdown holds.
const HELD: [&str; 11] = [
    keys::BOLD,
    keys::ITALIC,
    keys::STRIKETHROUGH,
    keys::CODE,
    keys::LINK,
    keys::LINK_TITLE,
    keys::SOURCE,
    keys::PICTURE_SRC,
    keys::PICTURE_ALT,
    keys::PICTURE_TITLE,
    keys::PICTURE,
];

/// The marks Markdown can't put on spaces at either end of what they
/// mark.
const EDGED: [&str; 3] = [keys::BOLD, keys::ITALIC, keys::STRIKETHROUGH];

/// What's still to shape: an object, at a depth, a table's cell or not; or
/// an empty cell filling out a short row, aligned as its column is.
enum Next {
    Object(ObjectId, usize, bool),
    Empty(usize, String),
}

/// `document`'s story as Markdown holds it, `sheet` naming its styles:
/// its blocks in reading order, without what Markdown can't hold or
/// writes away, such as empty paragraphs between blocks, spaces at a
/// paragraph's ends, and marks on spaces at the ends of what they mark.
pub fn shape(document: &Document, sheet: &StyleSheet) -> Vec<Node> {
    shape_of(document, sheet, document.root())
}

/// The shape of block `id` of `document` and what's in it, its depth
/// counted from it; the story's own blocks, for the story.
pub(crate) fn shape_of(document: &Document, sheet: &StyleSheet, id: ObjectId) -> Vec<Node> {
    let mut out = Vec::new();
    let mut stack: Vec<Next> = Vec::new();
    match document.object(id) {
        Some(Object::Story(_)) => {
            push_children(document, sheet, id, 0, true, &mut stack, &mut out);
        }
        Some(_) => stack.push(Next::Object(id, 0, false)),
        None => {}
    }
    while let Some(next) = stack.pop() {
        let (id, depth, cell) = match next {
            Next::Object(id, depth, cell) => (id, depth, cell),
            Next::Empty(depth, align) => {
                out.push(Node {
                    depth,
                    form: empty_text(align),
                });
                continue;
            }
        };
        match document.object(id) {
            Some(Object::Paragraph(p)) => out.push(Node {
                depth,
                form: text_node(p, sheet, cell),
            }),
            Some(Object::Block(b)) => {
                let form = match b.kind {
                    Kind::List => Form::List(match b.props.get(keys::ORDERED) {
                        Some(Value::Bool(true)) => Some(match b.props.get(keys::START) {
                            Some(Value::Int(n)) => *n,
                            _ => 1,
                        }),
                        _ => None,
                    }),
                    Kind::Item => Form::Item(match b.props.get(keys::TASK) {
                        Some(Value::Bool(done)) => Some(*done),
                        _ => None,
                    }),
                    Kind::Quote => Form::Quote,
                    Kind::Table => Form::Table,
                    Kind::Row => Form::Row,
                    Kind::Rule => Form::Rule,
                };
                out.push(Node { depth, form });
                if b.kind == Kind::Table {
                    // Rows as wide as the header, as Markdown reads them.
                    let header = b
                        .children
                        .first()
                        .map_or(&[][..], |&r| document.children(r));
                    let aligns: Vec<String> = header
                        .iter()
                        .map(|&c| {
                            match document.paragraph(c).and_then(|p| p.props.get(keys::ALIGN)) {
                                Some(Value::Text(a)) => a.trim().to_string(),
                                _ => String::new(),
                            }
                        })
                        .collect();
                    for &row in b.children.iter().rev() {
                        let cells = document.children(row);
                        for column in (cells.len()..aligns.len()).rev() {
                            stack.push(Next::Empty(depth + 2, aligns[column].clone()));
                        }
                        for &c in cells.iter().take(aligns.len()).rev() {
                            stack.push(Next::Object(c, depth + 2, true));
                        }
                        stack.push(Next::Object(row, depth + 1, false));
                    }
                } else if b.kind != Kind::Row {
                    let holds_one = matches!(b.kind, Kind::Item | Kind::Quote);
                    push_children(
                        document,
                        sheet,
                        id,
                        depth + 1,
                        holds_one,
                        &mut stack,
                        &mut out,
                    );
                }
            }
            Some(Object::Story(_) | Object::Sheet(_) | Object::Style(_)) | None => {}
        }
    }
    out
}

/// An empty paragraph, or an empty cell aligned `align`.
fn empty_text(align: String) -> Form {
    Form::Text {
        kind: String::new(),
        detail: align,
        text: String::new(),
        runs: Vec::new(),
    }
}

/// Pushes the blocks of container `id` that Markdown holds onto `stack`,
/// to come next, at `depth`: without empty paragraphs, lists or tables;
/// and if `holds_one` and none's left, an empty paragraph, as an empty
/// list item or quote has.
fn push_children(
    document: &Document,
    sheet: &StyleSheet,
    id: ObjectId,
    depth: usize,
    holds_one: bool,
    stack: &mut Vec<Next>,
    out: &mut Vec<Node>,
) {
    let kept: Vec<ObjectId> = document
        .children(id)
        .iter()
        .copied()
        .filter(|&c| match document.object(c) {
            Some(Object::Paragraph(p)) => !vanishes(p, sheet),
            Some(Object::Block(b)) => {
                !(matches!(b.kind, Kind::List | Kind::Table) && b.children.is_empty())
            }
            _ => false,
        })
        .collect();
    if kept.is_empty() && holds_one {
        out.push(Node {
            depth,
            form: empty_text(String::new()),
        });
        return;
    }
    stack.extend(
        kept.into_iter()
            .rev()
            .map(|c| Next::Object(c, depth, false)),
    );
}

/// Whether Markdown writes `p` as nothing: a paragraph of nothing but
/// spaces, or Markdown kept as it was that's empty.
pub(crate) fn vanishes(p: &Paragraph, sheet: &StyleSheet) -> bool {
    match text_kind(p, sheet).as_str() {
        "" => canonical(p).0.is_empty(),
        "source" => trimmed(&p.text).is_empty(),
        _ => false,
    }
}

/// What kind of text a paragraph is, as [`Form::Text`] names it.
pub(crate) fn text_kind(p: &Paragraph, sheet: &StyleSheet) -> String {
    let style = sheet.style_of(p);
    if style.code {
        if matches!(p.props.get(keys::SOURCE), Some(Value::Text(_))) {
            return "source".to_string();
        }
        return "code".to_string();
    }
    match style.heading {
        Some(level) if (1..=6).contains(&level) => format!("h{level}"),
        _ => String::new(),
    }
}

/// `text` without what a paragraph's ends lose in Markdown: spaces, tabs
/// and line breaks.
fn trimmed(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\u{2028}'])
}

/// A paragraph as Markdown holds it.
fn text_node(p: &Paragraph, sheet: &StyleSheet, cell: bool) -> Form {
    let kind = if cell {
        String::new()
    } else {
        text_kind(p, sheet)
    };
    let text_prop = |key: &str| match p.props.get(key) {
        Some(Value::Text(t)) => t.trim().to_string(),
        _ => String::new(),
    };
    let detail = match kind.as_str() {
        "code" => text_prop(keys::INFO),
        "source" => text_prop(keys::SOURCE),
        "" if cell => text_prop(keys::ALIGN),
        _ => String::new(),
    };
    if kind == "code" || kind == "source" {
        // Code and kept Markdown are what they say, without marks.
        return Form::Text {
            kind,
            detail,
            text: p.text.clone(),
            runs: Vec::new(),
        };
    }
    let (text, chars) = canonical(p);
    let mut runs: Vec<(u32, Vec<(String, Value)>)> = Vec::new();
    for props in chars {
        let props: Vec<(String, Value)> = props.into_iter().collect();
        match runs.last_mut() {
            Some((n, last)) if *last == props => *n += 1,
            _ => runs.push((1, props)),
        }
    }
    Form::Text {
        kind,
        detail,
        text,
        runs,
    }
}

/// A paragraph's text and each of its characters' properties as Markdown
/// holds them: spaces and line breaks gone from its ends and after its
/// line breaks; marks gone from the spaces at the ends of what they mark;
/// a picture's characters with only what Markdown says of pictures, and
/// pictures nowhere else.
pub(crate) fn canonical(p: &Paragraph) -> (String, Vec<Props>) {
    let mut chars: Vec<(char, Props)> = Vec::with_capacity(p.text.len());
    let mut text = p.text.chars();
    for run in &p.runs {
        let held: Props = run
            .props
            .iter()
            .filter(|(k, v)| {
                HELD.contains(&k.as_str())
                    && !matches!(v, Value::Bool(false))
                    && !(k.as_str() == keys::LINK_TITLE
                        && matches!(v, Value::Text(t) if t.is_empty()))
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for _ in 0..run.len {
            let Some(c) = text.next() else { break };
            let mut props = held.clone();
            if c == OBJECT && props.contains_key(keys::PICTURE) {
                // A picture is its link, Markdown's way, and not code.
                props.remove(keys::PICTURE);
                props.remove(keys::CODE);
                props.remove(keys::SOURCE);
            } else {
                props.retain(|k, _| !k.starts_with("picture"));
            }
            if !props.contains_key(keys::LINK) {
                props.remove(keys::LINK_TITLE);
            }
            chars.push((c, props));
        }
    }
    // Spaces after line breaks, and at the ends, go, but for those in
    // code or kept Markdown, which are its own.
    let space = |c: char| c == ' ' || c == '\t';
    let own = |props: &Props| props.contains_key(keys::CODE) || props.contains_key(keys::SOURCE);
    let mut kept: Vec<(char, Props)> = Vec::with_capacity(chars.len());
    for (c, props) in chars {
        let after_break = kept.last().is_some_and(|(l, _)| *l == '\u{2028}');
        let loose = (space(c) || c == '\u{2028}') && !own(&props);
        if loose && (kept.is_empty() || (after_break && space(c))) {
            continue;
        }
        kept.push((c, props));
    }
    while kept
        .last()
        .is_some_and(|(c, props)| (space(*c) || *c == '\u{2028}') && !own(props))
    {
        kept.pop();
    }
    // Marks on spaces at the ends of what they mark go, but for what's
    // code or kept as written, whose spaces are its own.
    for key in EDGED {
        let mut i = 0;
        while i < kept.len() {
            if !kept[i].1.contains_key(key) {
                i += 1;
                continue;
            }
            let start = i;
            while i < kept.len() && kept[i].1.contains_key(key) {
                i += 1;
            }
            let end = i;
            let loose = |(c, props): &(char, Props)| {
                (space(*c) || *c == '\u{2028}')
                    && !props.contains_key(keys::CODE)
                    && !props.contains_key(keys::SOURCE)
            };
            let mut a = start;
            while a < end && loose(&kept[a]) {
                kept[a].1.remove(key);
                a += 1;
            }
            let mut b = end;
            while b > a && loose(&kept[b - 1]) {
                kept[b - 1].1.remove(key);
                b -= 1;
            }
        }
    }
    let text = kept.iter().map(|(c, _)| *c).collect();
    (text, kept.into_iter().map(|(_, p)| p).collect())
}
