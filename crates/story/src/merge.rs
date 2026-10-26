//! Bringing one document's styles into another's, matched by role and by
//! name (docs/ARCHITECTURE.md, section 6): as pasting from another
//! document does, where the destination's styles win and those it lacks
//! come along with their chains; and as replacing a document's stylesheet
//! with another does, or taking a newer version of the one it came from
//! where the version it copied can't be had (`crate::update`), where the
//! incoming sheet's win. Nothing is renamed "Body 2": a style that matches
//! is the same style.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use doc::{Document, ObjectId, Op, Props, Style, StyleKind, Value};

use crate::keys;
use crate::sheet::align_name;

pub(crate) fn text<'a>(props: &'a Props, key: &str) -> Option<&'a str> {
    match props.get(key) {
        Some(Value::Text(t)) => Some(t),
        _ => None,
    }
}

/// The style of `document` that incoming `style` matches: one of its kind
/// with its role, or else with its name, not already `taken`.
pub(crate) fn matching(
    document: &Document,
    style: &Style,
    taken: &BTreeSet<ObjectId>,
) -> Option<ObjectId> {
    let candidates = || {
        document.styles().iter().copied().filter(|id| {
            !taken.contains(id) && document.style(*id).is_some_and(|s| s.kind == style.kind)
        })
    };
    let props_of = |id: ObjectId| document.style(id).map(|s| &s.props);
    if let Some(role) = text(&style.props, keys::ROLE)
        && let Some(found) =
            candidates().find(|&id| props_of(id).and_then(|p| text(p, keys::ROLE)) == Some(role))
    {
        return Some(found);
    }
    let name = text(&style.props, keys::NAME)?;
    candidates().find(|&id| props_of(id).and_then(|p| text(p, keys::NAME)) == Some(name))
}

/// `props` with every reference to an incoming object turned into a
/// reference to what it became, by `map`; one to anything else is left
/// out, so it can't name what the destination means otherwise.
pub(crate) fn mapped(props: &Props, map: &HashMap<ObjectId, ObjectId>) -> Props {
    props
        .iter()
        .filter_map(|(k, v)| match v {
            Value::Ref(id) => map.get(id).map(|to| (k.clone(), Value::Ref(*to))),
            other => Some((k.clone(), other.clone())),
        })
        .collect()
}

/// Applies `op` to `document` and keeps it in `ops` if it applies.
pub(crate) fn keep(document: &mut Document, ops: &mut Vec<Op>, op: Op) {
    if document.apply(&op).is_ok() {
        ops.push(op);
    }
}

/// Brings `incoming`'s styles and parts into `document`'s stylesheet, which
/// it changes: each incoming style matched by role and then by name with
/// one of `document`'s of its kind, and those that match none added, each
/// after the styles it's based on. With `theirs`, the incoming sheet wins:
/// matched styles take its definitions, bases and names, and the sheet its
/// defaults and what it came from; without, the document's do, as pasting
/// has it. Returns the operations that made the change, which apply to
/// `document` as it was, and what each incoming style and part became.
pub fn merge(
    document: &mut Document,
    incoming: &Document,
    theirs: bool,
) -> (Vec<Op>, HashMap<ObjectId, ObjectId>) {
    let mut ops = Vec::new();
    let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
    let (Some(sheet), Some(their_sheet)) = (document.sheet(), incoming.sheet()) else {
        return (ops, map);
    };
    // What each incoming style becomes: the document's that matches it,
    // or a new one.
    let mut taken = BTreeSet::new();
    let mut fresh: Vec<ObjectId> = Vec::new();
    for &id in incoming.styles() {
        let Some(style) = incoming.style(id) else {
            continue;
        };
        match matching(document, style, &taken) {
            Some(found) => {
                taken.insert(found);
                map.insert(id, found);
            }
            None => {
                let new = document.new_id();
                map.insert(id, new);
                fresh.push(id);
            }
        }
    }
    map.insert(their_sheet, sheet);
    let matched: Vec<(ObjectId, ObjectId)> = incoming
        .styles()
        .iter()
        .filter(|id| !fresh.contains(id))
        .filter_map(|id| Some((*id, *map.get(id)?)))
        .collect();
    // Their bases let go first, so none loops while the rest change.
    if theirs {
        for &(from, to) in &matched {
            let want = incoming
                .style(from)
                .and_then(|s| s.base)
                .and_then(|b| map.get(&b).copied());
            if document
                .style(to)
                .is_some_and(|s| s.base.is_some() && s.base != want)
            {
                keep(
                    document,
                    &mut ops,
                    Op::SetBase {
                        style: to,
                        base: None,
                    },
                );
            }
        }
    }
    // Those the document lacks, each after the styles it's based on.
    let mut pending = fresh;
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|&id| {
            let Some(style) = incoming.style(id) else {
                return false;
            };
            let base = style.base.and_then(|b| map.get(&b).copied());
            if base.is_some_and(|b| document.style(b).is_none()) {
                return true;
            }
            let op = Op::InsertStyle {
                sheet,
                after: document.styles().last().copied(),
                id: map[&id],
                style: Style {
                    sheet,
                    kind: style.kind,
                    base,
                    props: mapped(&style.props, &map),
                },
            };
            keep(document, &mut ops, op);
            false
        });
        if pending.len() == before {
            break;
        }
    }
    if theirs {
        for &(from, to) in &matched {
            let (Some(theirs), Some(ours)) = (incoming.style(from), document.style(to)) else {
                continue;
            };
            let want = mapped(&theirs.props, &map);
            let have = ours.props.clone();
            set_props(document, &mut ops, to, &have, &want);
            let base = theirs.base.and_then(|b| map.get(&b).copied());
            if document.style(to).is_some_and(|s| s.base != base) {
                keep(document, &mut ops, Op::SetBase { style: to, base });
            }
        }
        if let (Some(ours), Some(theirs)) = (document.stylesheet(), incoming.stylesheet()) {
            let have = ours.props.clone();
            let want = mapped(&theirs.props, &map);
            set_props(document, &mut ops, sheet, &have, &want);
        }
    }
    (ops, map)
}

/// Sets `style`'s properties from `have` to `want`.
fn set_props(
    document: &mut Document,
    ops: &mut Vec<Op>,
    style: ObjectId,
    have: &Props,
    want: &Props,
) {
    let keys: BTreeSet<&String> = have.keys().chain(want.keys()).collect();
    for key in keys {
        if have.get(key) != want.get(key) {
            keep(
                document,
                ops,
                Op::SetStyleProp {
                    style,
                    key: key.clone(),
                    value: want.get(key).cloned(),
                },
            );
        }
    }
}

/// A difference between a document's styles and another stylesheet's, as
/// a person reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The style or part it's in, by name, or "Defaults".
    pub style: String,
    /// What differs: "size 20 pt, not 22 pt", or "new".
    pub what: String,
}

/// A value as a person reads it: lengths in points, references by name.
pub(crate) fn show(document: &Document, key: &str, value: Option<&Value>) -> String {
    let lengths = [
        keys::TAB_INTERVAL,
        keys::QUOTE_INDENT,
        keys::QUOTE_BAR,
        keys::QUOTE_SPACE,
        keys::CODE_PADDING,
        keys::TABLE_SPACE,
        keys::CELL_PADDING,
        keys::RULE,
        keys::HAIRLINE,
        keys::RULE_SPACE,
        keys::SIZE,
        keys::SPACE_BEFORE,
        keys::SPACE_AFTER,
        keys::INDENT_START,
        keys::INDENT_END,
        keys::INDENT_FIRST,
        keys::PAGE_WIDTH,
        keys::PAGE_HEIGHT,
        keys::MARGIN_TOP,
        keys::MARGIN_BOTTOM,
        keys::MARGIN_LEFT,
        keys::MARGIN_RIGHT,
        keys::LIST_INDENT,
        keys::MARKER_GAP,
    ];
    match value {
        None => "nothing".to_string(),
        Some(Value::Int(n)) if lengths.contains(&key) => {
            let pt = *n as f64 / text::units::POINT as f64;
            format!("{} pt", (pt * 100.0).round() / 100.0)
        }
        Some(Value::Int(n)) => n.to_string(),
        Some(Value::Bool(b)) => if *b { "on" } else { "off" }.to_string(),
        Some(Value::Text(t)) if key == keys::ALIGN => {
            crate::sheet::align_of(t).map_or_else(|| t.clone(), |a| align_name(a).to_string())
        }
        Some(Value::Text(t)) => t.clone(),
        Some(Value::Ref(id)) => document
            .style(*id)
            .and_then(|s| text(&s.props, keys::NAME))
            .unwrap_or("something gone")
            .to_string(),
    }
}

/// What taking `incoming`'s stylesheet would change of `document`'s,
/// matched as [`merge`] matches styles: each value that differs, in style
/// order, and each style the document lacks. Its own styles that the
/// other sheet hasn't stay as they are, so they aren't changes.
pub fn changes(document: &Document, incoming: &Document) -> Vec<Change> {
    let mut out = Vec::new();
    let mut taken = BTreeSet::new();
    let name = |d: &Document, id: ObjectId| {
        d.style(id)
            .and_then(|s| text(&s.props, keys::NAME))
            .unwrap_or_default()
            .to_string()
    };
    let differ = |out: &mut Vec<Change>, style: String, ours: &Props, theirs: &Props| {
        let keys: BTreeMap<&String, ()> =
            ours.keys().chain(theirs.keys()).map(|k| (k, ())).collect();
        for key in keys.keys() {
            if [keys::ORIGIN, keys::ORIGIN_VERSION].contains(&key.as_str()) {
                continue;
            }
            let (a, b) = (
                show(incoming, key, theirs.get(*key)),
                show(document, key, ours.get(*key)),
            );
            if a != b {
                out.push(Change {
                    style: style.clone(),
                    what: format!("{key} {a}, not {b}"),
                });
            }
        }
    };
    if let (Some(ours), Some(theirs)) = (document.stylesheet(), incoming.stylesheet()) {
        differ(&mut out, "Defaults".to_string(), &ours.props, &theirs.props);
    }
    for &id in incoming.styles() {
        let Some(style) = incoming.style(id) else {
            continue;
        };
        match matching(document, style, &taken) {
            Some(found) => {
                taken.insert(found);
                let ours = document.style(found).expect("a style");
                differ(&mut out, name(document, found), &ours.props, &style.props);
                let base = |d: &Document, s: &Style| s.base.map(|b| name(d, b));
                if base(incoming, style) != base(document, ours) {
                    out.push(Change {
                        style: name(document, found),
                        what: format!(
                            "based on {}",
                            base(incoming, style).unwrap_or_else(|| "nothing".to_string())
                        ),
                    });
                }
            }
            None => out.push(Change {
                style: name(incoming, id),
                what: format!("a new {}", kind_name(style.kind)),
            }),
        }
    }
    out
}

fn kind_name(kind: StyleKind) -> &'static str {
    kind.name()
}
