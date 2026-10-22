//! Properties, and runs of character properties along a paragraph's text.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A property's value. Lengths are integers in layout units
/// (docs/ARCHITECTURE.md, section 9). Files store values by their variants'
/// positions (spec/format/README.md): new variants go at the end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    /// On or off, such as bold.
    Bool(bool),
    /// A number, such as a size in layout units.
    Int(i64),
    /// Text, such as a font family or a style's name.
    Text(String),
}

/// A set of properties by name: a paragraph's, or a run of text's. A
/// property that isn't set takes its value from the style.
pub type Props = BTreeMap<String, Value>;

/// A stretch of text with the same character properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// Its length, in Unicode scalar values.
    pub len: u32,
    /// Its properties, shared between runs and snapshots.
    pub props: Arc<Props>,
}

impl Run {
    /// A run of `len` characters with `props`.
    pub fn new(len: u32, props: Props) -> Run {
        Run {
            len,
            props: Arc::new(props),
        }
    }
}

/// The total length of `runs`.
pub(crate) fn total(runs: &[Run]) -> u32 {
    runs.iter().map(|r| r.len).sum()
}

/// Drops empty runs and merges neighbours with equal properties, so the
/// same formatting always has the same runs.
pub(crate) fn normalize(runs: &mut Vec<Run>) {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        if run.len == 0 {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.props == run.props => last.len += run.len,
            _ => out.push(run),
        }
    }
    *runs = out;
}

/// The runs covering `start..start + len`.
pub(crate) fn slice(runs: &[Run], start: u32, len: u32) -> Vec<Run> {
    let end = start + len;
    let mut out = Vec::new();
    let mut pos = 0;
    for run in runs {
        let (a, b) = (pos.max(start), (pos + run.len).min(end));
        if a < b {
            out.push(Run {
                len: b - a,
                props: run.props.clone(),
            });
        }
        pos += run.len;
    }
    out
}

/// `runs` with `start..start + remove` replaced by `insert`.
pub(crate) fn splice(runs: &[Run], start: u32, remove: u32, insert: &[Run]) -> Vec<Run> {
    let total = total(runs);
    let mut out = slice(runs, 0, start);
    out.extend_from_slice(insert);
    out.extend(slice(runs, start + remove, total - start - remove));
    normalize(&mut out);
    out
}

/// The values `key` has along `start..start + len`, as (length, value)
/// stretches, merged.
pub(crate) fn values(runs: &[Run], start: u32, len: u32, key: &str) -> Vec<(u32, Option<Value>)> {
    let mut out: Vec<(u32, Option<Value>)> = Vec::new();
    for run in slice(runs, start, len) {
        let value = run.props.get(key).cloned();
        match out.last_mut() {
            Some(last) if last.1 == value => last.0 += run.len,
            _ => out.push((run.len, value)),
        }
    }
    out
}

/// `runs` with `key` set to `value` (or unset, for `None`) along
/// `start..start + len`.
pub(crate) fn set(
    runs: &[Run],
    start: u32,
    len: u32,
    key: &str,
    value: &Option<Value>,
) -> Vec<Run> {
    let changed: Vec<Run> = slice(runs, start, len)
        .into_iter()
        .map(|run| {
            let mut props = (*run.props).clone();
            match value {
                Some(v) => props.insert(key.to_string(), v.clone()),
                None => props.remove(key),
            };
            Run::new(run.len, props)
        })
        .collect();
    splice(runs, start, len, &changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(len: u32) -> Run {
        Run::new(len, Props::from([("bold".into(), Value::Bool(true))]))
    }

    fn plain(len: u32) -> Run {
        Run::new(len, Props::new())
    }

    #[test]
    fn normalize_merges_and_drops() {
        let mut runs = vec![plain(2), plain(0), plain(3), bold(1), bold(2), plain(1)];
        normalize(&mut runs);
        assert_eq!(runs, [plain(5), bold(3), plain(1)]);
    }

    #[test]
    fn set_and_values_round_trip() {
        let runs = vec![plain(10)];
        let runs = set(&runs, 2, 3, "bold", &Some(Value::Bool(true)));
        assert_eq!(runs, [plain(2), bold(3), plain(5)]);
        assert_eq!(
            values(&runs, 1, 5, "bold"),
            [(1, None), (3, Some(Value::Bool(true))), (1, None)]
        );
        let runs = set(&runs, 0, 10, "bold", &None);
        assert_eq!(runs, [plain(10)]);
    }

    #[test]
    fn splice_replaces() {
        let runs = vec![plain(4), bold(4)];
        assert_eq!(splice(&runs, 3, 2, &[bold(1)]), [plain(3), bold(4)]);
        assert_eq!(splice(&runs, 0, 8, &[]), []);
    }
}
