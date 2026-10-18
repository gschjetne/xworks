//! Runs the General Decimal Arithmetic test cases for decimal128 (the
//! `dq*.decTest` files) against a candidate.
//!
//! A case passes *exactly* when the candidate's result, in scientific
//! notation, is the expected text, digits and exponent alike; and passes
//! *by value* when it equals the expected number, NaNs matching NaNs and
//! zeros matching in sign. Cases are skipped when they need a context other
//! than decimal128's defaults (34 digits, rounding half to even), or an
//! operand the candidate can't read, such as a NaN with a payload or a raw
//! encoding.

use std::cmp::Ordering;
use std::path::Path;

use crate::candidates::Number;

/// The operations compared, and their test files.
pub const FILES: &[(&str, &str)] = &[
    ("add", "dqAdd.decTest"),
    ("subtract", "dqSubtract.decTest"),
    ("multiply", "dqMultiply.decTest"),
    ("divide", "dqDivide.decTest"),
    ("compare", "dqCompare.decTest"),
];

/// One file's results for one candidate.
#[derive(Debug, Default)]
pub struct Tally {
    pub exact: usize,
    pub by_value: usize,
    pub failed: usize,
    pub skipped: usize,
    /// The first few failures: the case, what was expected, what came.
    pub examples: Vec<String>,
}

/// Runs `file` (operation `op`) against `N`.
pub fn run<N: Number>(op: &str, file: &Path) -> std::io::Result<Tally> {
    let text = std::fs::read_to_string(file)?;
    let mut tally = Tally::default();
    let mut precision = 34;
    let mut rounding = String::from("half_even");
    let mut ctx = N::ctx();
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
            || operands.iter().any(|o| o.starts_with('#'))
            || expected.starts_with('#')
            || operands.iter().any(|o| has_payload(o))
            || has_payload(expected)
        {
            tally.skipped += 1;
            continue;
        }
        let (Some(a), Some(b)) = (
            N::parse(&mut ctx, &operands[0]),
            N::parse(&mut ctx, &operands[1]),
        ) else {
            tally.skipped += 1;
            continue;
        };
        let (got_text, matched) = if op == "compare" {
            let got = match N::compare(&mut ctx, a, b) {
                Some(Ordering::Less) => "-1",
                Some(Ordering::Equal) => "0",
                Some(Ordering::Greater) => "1",
                None => "NaN",
            };
            let want = expected.trim_start_matches('+').to_ascii_lowercase();
            let want = if want.ends_with("nan") {
                "nan".to_string()
            } else {
                want
            };
            (got.to_string(), got.to_ascii_lowercase() == want)
        } else {
            let got = match op {
                "add" => N::add(&mut ctx, a, b),
                "subtract" => N::sub(&mut ctx, a, b),
                "multiply" => N::mul(&mut ctx, a, b),
                "divide" => N::div(&mut ctx, a, b),
                _ => unreachable!("only the operations in FILES are run"),
            };
            let text = got.text();
            if text.eq_ignore_ascii_case(expected) {
                tally.exact += 1;
                continue;
            }
            let matched = match N::parse(&mut ctx, expected) {
                Some(want) if want.is_nan() => got.is_nan(),
                Some(want) => {
                    N::compare(&mut ctx, got, want) == Some(Ordering::Equal)
                        && got.is_negative() == want.is_negative()
                }
                None => false,
            };
            (text, matched)
        };
        if matched {
            if op == "compare" {
                tally.exact += 1;
            } else {
                tally.by_value += 1;
            }
        } else {
            tally.failed += 1;
            if tally.examples.len() < 4 {
                tally.examples.push(format!(
                    "{} {} {} -> {} (got {})",
                    tokens[0], operands[0], operands[1], expected, got_text
                ));
            }
        }
    }
    Ok(tally)
}

/// Whether a NaN operand carries a payload, as in `NaN123`.
fn has_payload(text: &str) -> bool {
    let lower = text.trim_start_matches(['+', '-']).to_ascii_lowercase();
    (lower.starts_with("nan") && lower.len() > 3) || (lower.starts_with("snan") && lower.len() > 4)
}

/// Splits a line into tokens, keeping quoted operands whole and dropping
/// `--` comments.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '-' && line_rest_starts_comment(&mut chars.clone()) {
            break;
        } else if c == '\'' || c == '"' {
            let quote = c;
            chars.next();
            let mut token = String::new();
            while let Some(c) = chars.next() {
                if c == quote {
                    if chars.peek() == Some(&quote) {
                        token.push(quote);
                        chars.next();
                    } else {
                        break;
                    }
                } else {
                    token.push(c);
                }
            }
            tokens.push(token);
        } else {
            let mut token = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                token.push(c);
                chars.next();
            }
            tokens.push(token);
        }
    }
    tokens
}

fn line_rest_starts_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    chars.next() == Some('-') && chars.next() == Some('-')
}
