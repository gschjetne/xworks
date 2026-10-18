//! calc's decimal numbers against the General Decimal Arithmetic test cases
//! for decimal128 (Mike Cowlishaw, IBM): the `dq*.decTest` files for
//! addition, subtraction, multiplication, division and comparison.
//!
//! Their license is unclear, so CI fetches them rather than committing them
//! (CLAUDE.md, rule 10). Point `XWORKS_DECTEST` at the folder of
//! `.decTest` files and run:
//!
//! ```sh
//! cargo nextest run -p calc --test dectest --run-ignored only
//! ```
//!
//! Every case must match exactly, digits and exponent alike, except those
//! that need what calc's numbers don't keep: a context other than
//! decimal128's defaults (34 digits, rounding half to even), NaN payloads,
//! or raw encodings.

#![allow(
    clippy::disallowed_methods,
    reason = "the pure-crate rule is for calc itself; its tests may read the fetched test files"
)]

use std::cmp::Ordering;
use std::path::PathBuf;

use calc::decimal::{Decimal, Status};

const FILES: &[(&str, &str)] = &[
    ("add", "dqAdd.decTest"),
    ("subtract", "dqSubtract.decTest"),
    ("multiply", "dqMultiply.decTest"),
    ("divide", "dqDivide.decTest"),
    ("compare", "dqCompare.decTest"),
];

#[test]
#[ignore = "needs the fetched test cases in XWORKS_DECTEST"]
fn decimal128_test_cases_pass() {
    let dir = PathBuf::from(
        std::env::var_os("XWORKS_DECTEST").expect("XWORKS_DECTEST names the .decTest folder"),
    );
    let mut failures = Vec::new();
    let mut passed = 0;
    for (op, file) in FILES {
        let text = std::fs::read_to_string(dir.join(file)).expect("the test file reads");
        let (pass, fails) = run(op, &text);
        passed += pass;
        failures.extend(fails);
    }
    assert!(
        passed > 2000,
        "only {passed} cases ran; are the files right?"
    );
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        passed + failures.len(),
        failures.join("\n")
    );
}

/// Runs one file's cases of operation `op`; returns how many passed and a
/// line for each failure.
fn run(op: &str, text: &str) -> (usize, Vec<String>) {
    let mut passed = 0;
    let mut failures = Vec::new();
    let mut precision = 34;
    let mut rounding = String::from("half_even");
    for line in text.lines() {
        let tokens = tokenize(line);
        if tokens.is_empty() {
            continue;
        }
        if let Some(key) = tokens[0].strip_suffix(':') {
            match key.to_ascii_lowercase().as_str() {
                "precision" => precision = tokens[1].parse().unwrap_or(0),
                "rounding" => rounding = tokens[1].to_ascii_lowercase(),
                _ => {}
            }
            continue;
        }
        let Some(arrow) = tokens.iter().position(|t| t == "->") else {
            continue;
        };
        if tokens.len() < 3 || !tokens[1].eq_ignore_ascii_case(op) {
            continue;
        }
        let operands = &tokens[2..arrow];
        let expected = &tokens[arrow + 1];
        if precision != 34
            || rounding != "half_even"
            || operands.len() != 2
            || operands
                .iter()
                .chain([expected])
                .any(|t| t.starts_with('#') || has_payload(t))
        {
            continue;
        }
        let (Some(a), Some(b)) = (Decimal::parse(&operands[0]), Decimal::parse(&operands[1]))
        else {
            failures.push(format!(
                "{}: an operand doesn't parse: {operands:?}",
                tokens[0]
            ));
            continue;
        };
        let mut status = Status::default();
        let got = match op {
            "add" => a.add(b, &mut status).to_string(),
            "subtract" => a.sub(b, &mut status).to_string(),
            "multiply" => a.mul(b, &mut status).to_string(),
            "divide" => a.div(b, &mut status).to_string(),
            "compare" => match a.compare(b) {
                Some(Ordering::Less) => "-1".into(),
                Some(Ordering::Equal) => "0".into(),
                Some(Ordering::Greater) => "1".into(),
                // The NaN that propagates: the first signaling NaN, else
                // the first NaN, with its sign.
                None => {
                    let nan = if a.is_signaling() || (a.is_nan() && !b.is_signaling()) {
                        a
                    } else {
                        b
                    };
                    if nan.is_negative() { "-NaN" } else { "NaN" }.into()
                }
            },
            _ => unreachable!("only the operations in FILES run"),
        };
        if got == *expected {
            passed += 1;
        } else {
            failures.push(format!(
                "{} {} {} -> {} (got {got})",
                tokens[0], operands[0], operands[1], expected
            ));
        }
    }
    (passed, failures)
}

/// Whether a NaN carries a payload, as in `NaN123`.
fn has_payload(text: &str) -> bool {
    let lower = text.trim_start_matches(['+', '-']).to_ascii_lowercase();
    (lower.starts_with("nan") && lower.len() > 3) || (lower.starts_with("snan") && lower.len() > 4)
}

/// Splits a line into tokens, keeping quoted operands whole and dropping
/// `--` comments.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        if rest.starts_with("--") {
            break;
        }
        if let Some(quote) = rest.chars().next().filter(|c| *c == '\'' || *c == '"') {
            let mut token = String::new();
            let mut chars = rest[1..].char_indices().peekable();
            let mut end = rest.len();
            while let Some((i, c)) = chars.next() {
                if c == quote {
                    if chars.peek().map(|&(_, c)| c) == Some(quote) {
                        token.push(quote);
                        chars.next();
                    } else {
                        end = i + 2;
                        break;
                    }
                } else {
                    token.push(c);
                }
            }
            tokens.push(token);
            rest = rest[end..].trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            tokens.push(rest[..end].to_string());
            rest = rest[end..].trim_start();
        }
    }
    tokens
}
