//! Taking a newer version of the stylesheet a document copied, three-way
//! (docs/ARCHITECTURE.md, section 6; review sjj5ak5a): the version it
//! copied, the base; the newer one; and the document's own sheet. Styles
//! and parts are matched between them by role, then by name, as pasting
//! matches them.
//!
//! - What the newer version changed since the base, the document takes,
//!   where it hasn't changed that value itself.
//! - A value the document changed, and the newer version didn't, stays.
//! - Where both changed the same value, differently, the document's
//!   stays, and the update says so.
//! - Styles and parts the newer version adds come in, after the style
//!   before them in its order.
//! - One it removes goes only if nothing uses it and the document didn't
//!   change it; otherwise it stays, and the update says so.
//! - The document then records the newer version as its origin.
//!
//! Where the base can't be had, [`update`] falls back to comparing the
//! document's sheet with the newer one alone, as [`crate::merge`] does
//! with the incoming sheet winning, and says so.

use std::collections::{BTreeSet, HashMap};

use doc::{Document, ObjectId, Op, Props, Style, Value};

use crate::keys;
use crate::merge::{Change, changes, keep, mapped, matching, merge, show, text};

/// What taking a newer version of a document's stylesheet does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Update {
    /// The operations that make it, which apply to the document as it was,
    /// to commit as one step.
    pub ops: Vec<Op>,
    /// What the newer version changed that the document takes, as a
    /// person reads it, in the newer sheet's order.
    pub changes: Vec<Change>,
    /// What the newer version changed that the document keeps as it has
    /// it: values both changed, and styles it removed that the document
    /// uses or changed.
    pub kept: Vec<Change>,
    /// Whether it compared three ways, from the version the document
    /// copied; if not, the base wasn't to be had, and it compared the
    /// document's sheet with the newer one alone.
    pub three_way: bool,
}

/// A value as it compares across documents: its references mapped into
/// one document's IDs. One that maps to nothing there is lost, and equal
/// to nothing.
#[derive(Clone, Debug)]
enum Seen {
    Value(Value),
    Lost,
}

/// Whether two values, seen in the same document's IDs, are the same.
fn same(a: &Option<Seen>, b: &Option<Seen>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Seen::Value(x)), Some(Seen::Value(y))) => x == y,
        _ => false,
    }
}

/// `value` seen through `map`, or as it is without one.
fn seen(value: Option<&Value>, map: Option<&HashMap<ObjectId, ObjectId>>) -> Option<Seen> {
    Some(match (value?, map) {
        (Value::Ref(id), Some(map)) => match map.get(id) {
            Some(to) => Seen::Value(Value::Ref(*to)),
            None => Seen::Lost,
        },
        (other, _) => Seen::Value(other.clone()),
    })
}

/// A style's base seen through `map`, or as it is without one.
fn seen_base(base: Option<ObjectId>, map: Option<&HashMap<ObjectId, ObjectId>>) -> Option<Seen> {
    seen(base.map(Value::Ref).as_ref(), map)
}

/// What becomes of one value, three ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    /// The newer version didn't change it, or the document made the same
    /// change: the document's stays, and there's nothing to say.
    Leave,
    /// The newer version changed it and the document didn't: it takes the
    /// newer version's.
    Take,
    /// Both changed it, differently: the document's stays, and that's
    /// said.
    Keep,
}

/// What becomes of a value, given whether the newer version changed it
/// from the base, whether the document did, and whether the two agree.
fn way(newer_changed: bool, ours_changed: bool, ours_is_newer: bool) -> Way {
    if !newer_changed || ours_is_newer {
        Way::Leave
    } else if !ours_changed {
        Way::Take
    } else {
        Way::Keep
    }
}

/// What each style of `from` matches in `to`, in `from`'s order, as
/// [`matching`] matches them, each of `to`'s matched once at most; and
/// `from`'s sheet `to`'s.
fn matches(from: &Document, to: &Document) -> HashMap<ObjectId, ObjectId> {
    let mut map = HashMap::new();
    let mut taken = BTreeSet::new();
    for &id in from.styles() {
        let Some(style) = from.style(id) else {
            continue;
        };
        if let Some(found) = matching(to, style, &taken) {
            taken.insert(found);
            map.insert(id, found);
        }
    }
    if let (Some(a), Some(b)) = (from.sheet(), to.sheet()) {
        map.insert(a, b);
    }
    map
}

/// `map` turned around.
fn inverse(map: &HashMap<ObjectId, ObjectId>) -> HashMap<ObjectId, ObjectId> {
    map.iter().map(|(a, b)| (*b, *a)).collect()
}

/// The style's or part's name in `document`, or the sheet's, "Defaults".
fn name_of(document: &Document, id: ObjectId) -> String {
    if document.sheet() == Some(id) {
        return "Defaults".to_string();
    }
    document
        .style(id)
        .and_then(|s| text(&s.props, keys::NAME))
        .unwrap_or_default()
        .to_string()
}

/// The properties of style, part or sheet `id` of `document`.
fn props_of(document: &Document, id: ObjectId) -> Option<&Props> {
    if document.sheet() == Some(id) {
        return document.stylesheet().map(|s| &s.props);
    }
    document.style(id).map(|s| &s.props)
}

/// Whether anything in `document` refers to style or part `id`: a
/// paragraph, a run of text, a container, another style as its base or by
/// a value, or the sheet.
fn used(document: &Document, id: ObjectId) -> bool {
    let refers = |props: &Props| props.values().any(|v| *v == Value::Ref(id));
    let story = document
        .subtree(document.root())
        .into_iter()
        .any(|o| match document.object(o) {
            Some(doc::Object::Paragraph(p)) => {
                refers(&p.props) || p.runs.iter().any(|r| refers(&r.props))
            }
            Some(doc::Object::Block(b)) => refers(&b.props),
            _ => false,
        });
    story
        || document.styles().iter().any(|&s| {
            s != id
                && document
                    .style(s)
                    .is_some_and(|style| style.base == Some(id) || refers(&style.props))
        })
        || document.stylesheet().is_some_and(|s| refers(&s.props))
}

/// The keys a sheet's properties hold that say where it came from, which
/// an update sets at its end rather than compares.
const ORIGIN_KEYS: [&str; 2] = [keys::ORIGIN, keys::ORIGIN_VERSION];

/// Takes `newer`, a newer version of the stylesheet `document`'s was copied
/// from, into `document`, which it changes: three ways, from `base`, the
/// version it copied, as the module says; or with no base, two ways, the
/// newer version winning, as replacing the sheet does. Returns the
/// operations that made the change, which apply to `document` as it was,
/// and what they changed and kept.
pub fn update(document: &mut Document, base: Option<&Document>, newer: &Document) -> Update {
    let Some(base) = base else {
        let said = changes(document, newer);
        let (ops, _) = merge(document, newer, true);
        return Update {
            ops,
            changes: said,
            kept: Vec::new(),
            three_way: false,
        };
    };
    let mut out = Update {
        three_way: true,
        ..Update::default()
    };
    let (Some(sheet), Some(newer_sheet)) = (document.sheet(), newer.sheet()) else {
        return out;
    };
    // What matches what: the newer version's styles in the base's and the
    // document's, and the base's in the document's.
    let n_to_b = matches(newer, base);
    let mut n_to_d = matches(newer, document);
    let b_to_d = matches(base, document);
    let b_from_n = inverse(&n_to_b);
    // Those the document lacks get IDs of its own.
    let mut fresh: Vec<ObjectId> = Vec::new();
    for &id in newer.styles() {
        if let std::collections::hash_map::Entry::Vacant(slot) = n_to_d.entry(id) {
            slot.insert(document.new_id());
            fresh.push(id);
        }
    }
    n_to_d.insert(newer_sheet, sheet);

    // The changes said in the newer sheet's order: the defaults first,
    // then each style, new or matched.
    let mut said: Vec<(ObjectId, Change, bool)> = Vec::new();
    let mut say = |at: ObjectId, style: String, what: String, kept: bool| {
        said.push((at, Change { style, what }, kept));
    };

    // Bases that change let go first, so none loops while the rest change.
    let mut bases: Vec<(ObjectId, Option<ObjectId>)> = Vec::new();
    for &n in newer.styles() {
        let (Some(&d), Some(theirs)) = (n_to_d.get(&n), newer.style(n)) else {
            continue;
        };
        if fresh.contains(&n) {
            continue;
        }
        let Some(ours) = document.style(d) else {
            continue;
        };
        let b = n_to_b.get(&n).and_then(|&b| base.style(b));
        let base_base = b.and_then(|b| b.base);
        let newer_changed = !same(
            &seen_base(theirs.base, Some(&n_to_b)),
            &seen_base(base_base, None),
        );
        let ours_changed = !same(
            &seen_base(base_base, Some(&b_to_d)),
            &seen_base(ours.base, None),
        );
        let want = theirs.base.and_then(|x| n_to_d.get(&x).copied());
        let ours_is_newer = ours.base == want;
        let name = name_of(document, d);
        match way(newer_changed, ours_changed, ours_is_newer) {
            Way::Leave => {}
            Way::Take => {
                bases.push((d, want));
                let to = theirs
                    .base
                    .map_or_else(|| "nothing".to_string(), |x| name_of(newer, x));
                say(n, name, format!("based on {to}"), false);
            }
            Way::Keep => {
                let yours = ours
                    .base
                    .map_or_else(|| "nothing".to_string(), |x| name_of(document, x));
                say(
                    n,
                    name,
                    format!("based on {yours}, kept as the document has it"),
                    true,
                );
            }
        }
    }
    for &(d, _) in &bases {
        if document.style(d).is_some_and(|s| s.base.is_some()) {
            keep(
                document,
                &mut out.ops,
                Op::SetBase {
                    style: d,
                    base: None,
                },
            );
        }
    }

    // Those the newer version adds, each after its base, and after the
    // style before it in the newer sheet's order.
    let order: Vec<ObjectId> = newer.styles().to_vec();
    let mut pending = fresh.clone();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|&n| {
            let Some(style) = newer.style(n) else {
                return false;
            };
            let base_of = style.base.and_then(|b| n_to_d.get(&b).copied());
            if base_of.is_some_and(|b| document.style(b).is_none()) {
                return true;
            }
            let at = order.iter().position(|&x| x == n).unwrap_or(0);
            let after = order[..at]
                .iter()
                .rev()
                .filter_map(|x| n_to_d.get(x))
                .find(|d| document.style(**d).is_some())
                .copied();
            let op = Op::InsertStyle {
                sheet,
                after,
                id: n_to_d[&n],
                style: Style {
                    sheet,
                    kind: style.kind,
                    base: base_of,
                    props: mapped(&style.props, &n_to_d),
                },
            };
            keep(document, &mut out.ops, op);
            false
        });
        if pending.len() == before {
            break;
        }
    }
    for &n in &fresh {
        if document.style(n_to_d[&n]).is_some() {
            let kind = newer.style(n).map_or("style", |s| s.kind.name());
            say(n, name_of(newer, n), format!("a new {kind}"), false);
        }
    }

    // Each value of each style, part and the sheet both have.
    let mut pairs: Vec<ObjectId> = vec![newer_sheet];
    pairs.extend(newer.styles().iter().filter(|n| !fresh.contains(n)));
    for n in pairs {
        let Some(&d) = n_to_d.get(&n) else {
            continue;
        };
        let (Some(theirs), Some(ours)) = (props_of(newer, n), props_of(document, d)) else {
            continue;
        };
        let ours = ours.clone();
        let empty = Props::new();
        let b = n_to_b.get(&n).copied();
        let from = b.and_then(|b| props_of(base, b)).unwrap_or(&empty);
        let name = name_of(document, d);
        let keys: BTreeSet<&String> = from
            .keys()
            .chain(theirs.keys())
            .chain(ours.keys())
            .collect();
        for key in keys {
            if ORIGIN_KEYS.contains(&key.as_str()) {
                continue;
            }
            let newer_changed = !same(
                &seen(theirs.get(key), Some(&n_to_b)),
                &seen(from.get(key), None),
            );
            let ours_changed = !same(
                &seen(from.get(key), Some(&b_to_d)),
                &seen(ours.get(key), None),
            );
            let ours_is_newer = same(
                &seen(theirs.get(key), Some(&n_to_d)),
                &seen(ours.get(key), None),
            );
            match way(newer_changed, ours_changed, ours_is_newer) {
                Way::Leave => {}
                Way::Take => {
                    let value = theirs.get(key).and_then(|v| match v {
                        Value::Ref(id) => n_to_d.get(id).map(|to| Value::Ref(*to)),
                        other => Some(other.clone()),
                    });
                    let what = format!(
                        "{key} {}, not {}",
                        show(newer, key, theirs.get(key)),
                        show(document, key, ours.get(key))
                    );
                    let made = out.ops.len();
                    keep(
                        document,
                        &mut out.ops,
                        Op::SetStyleProp {
                            style: d,
                            key: key.clone(),
                            value,
                        },
                    );
                    if out.ops.len() > made {
                        say(n, name.clone(), what, false);
                    }
                }
                Way::Keep => {
                    let what = format!(
                        "{key} {}, kept as the document has it; the new version has {}",
                        show(document, key, ours.get(key)),
                        show(newer, key, theirs.get(key))
                    );
                    say(n, name.clone(), what, true);
                }
            }
        }
    }
    for (d, want) in bases {
        if document.style(d).is_some_and(|s| s.base != want) {
            keep(
                document,
                &mut out.ops,
                Op::SetBase {
                    style: d,
                    base: want,
                },
            );
        }
    }

    // Those the newer version removed: each goes if nothing uses it and
    // the document didn't change it, and stays, said, otherwise.
    for &b in base.styles() {
        if b_from_n.contains_key(&b) {
            continue;
        }
        let Some(&d) = b_to_d.get(&b) else {
            continue;
        };
        let (Some(was), Some(ours)) = (base.style(b), document.style(d)) else {
            continue;
        };
        let keys: BTreeSet<&String> = was.props.keys().chain(ours.props.keys()).collect();
        let changed = keys.iter().any(|key| {
            !same(
                &seen(was.props.get(*key), Some(&b_to_d)),
                &seen(ours.props.get(*key), None),
            )
        }) || !same(
            &seen_base(was.base, Some(&b_to_d)),
            &seen_base(ours.base, None),
        );
        let name = name_of(document, d);
        // Said where the base had it, after the newer style before it.
        let at = base.styles()[..base.styles().iter().position(|&x| x == b).unwrap_or(0)]
            .iter()
            .rev()
            .find_map(|x| b_from_n.get(x).copied())
            .unwrap_or(newer_sheet);
        if changed {
            say(
                at,
                name,
                "removed from the new version, kept as the document changed it".to_string(),
                true,
            );
        } else if used(document, d) {
            say(
                at,
                name,
                "removed from the new version, kept as the document uses it".to_string(),
                true,
            );
        } else {
            let made = out.ops.len();
            keep(document, &mut out.ops, Op::DeleteStyle { style: d });
            if out.ops.len() > made {
                say(at, name, "removed".to_string(), false);
            }
        }
    }

    // It now came from the newer version.
    if let Some(theirs) = newer.stylesheet() {
        let have = document
            .stylesheet()
            .map(|s| s.props.clone())
            .unwrap_or_default();
        for key in ORIGIN_KEYS {
            if have.get(key) != theirs.props.get(key) {
                keep(
                    document,
                    &mut out.ops,
                    Op::SetStyleProp {
                        style: sheet,
                        key: key.to_string(),
                        value: theirs.props.get(key).cloned(),
                    },
                );
            }
        }
    }

    // What was said, in the newer sheet's order.
    let rank = |n: ObjectId| -> usize {
        if n == newer_sheet {
            0
        } else {
            1 + order.iter().position(|&x| x == n).unwrap_or(order.len())
        }
    };
    said.sort_by_key(|(at, _, _)| rank(*at));
    for (_, change, kept) in said {
        if kept {
            out.kept.push(change);
        } else {
            out.changes.push(change);
        }
    }
    out
}
