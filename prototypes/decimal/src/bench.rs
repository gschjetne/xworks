//! Speed: arithmetic on money-like numbers, and a synthetic recalculation
//! of a million cells, each candidate against `f64`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use crate::candidates::Number;

/// Numbers as a sheet holds them: amounts with two decimals, between minus
/// and plus ten million, from a fixed seed so every run gets the same ones.
pub fn amounts(count: usize) -> Vec<String> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let cents = (state % 2_000_000_000) as i64 - 1_000_000_000;
            let sign = if cents < 0 { "-" } else { "" };
            let cents = cents.unsigned_abs();
            format!("{sign}{}.{:02}", cents / 100, cents % 100)
        })
        .collect()
}

/// One candidate's timings, each per operation, and the recalculation's
/// total.
pub struct Timings {
    pub name: &'static str,
    /// Nanoseconds per operation.
    pub parse: f64,
    pub add: f64,
    pub mul: f64,
    pub div: f64,
    pub recalc: Duration,
    /// The recalculation's last cell, to compare results across candidates.
    pub recalc_result: String,
}

/// Times `N` on `texts`, `rounds` times over for the arithmetic.
pub fn time<N: Number>(texts: &[String], rounds: usize) -> Timings {
    let mut ctx = N::ctx();
    let n = texts.len();

    let start = Instant::now();
    let values: Vec<N> = texts
        .iter()
        .map(|t| N::parse(&mut ctx, t).expect("amounts parse"))
        .collect();
    let parse = per_op(start, n);

    let start = Instant::now();
    for _ in 0..rounds {
        let mut sum = N::parse(&mut ctx, "0").unwrap();
        for &v in &values {
            sum = N::add(&mut ctx, sum, v);
        }
        black_box(sum);
    }
    let add = per_op(start, rounds * n);

    let start = Instant::now();
    for _ in 0..rounds {
        for pair in values.windows(2) {
            black_box(N::mul(&mut ctx, pair[0], pair[1]));
        }
    }
    let mul = per_op(start, rounds * (n - 1));

    let start = Instant::now();
    for _ in 0..rounds {
        for pair in values.windows(2) {
            black_box(N::div(&mut ctx, pair[0], pair[1]));
        }
    }
    let div = per_op(start, rounds * (n - 1));

    let (recalc, recalc_result) = recalc::<N>(&values);
    Timings {
        name: N::NAME,
        parse,
        add,
        mul,
        div,
        recalc,
        recalc_result,
    }
}

/// A million cells, as a recalculation after a change at the top of a long
/// model would run them: each cell is the one before it times a growth rate,
/// plus an amount; every thousandth cell is the sum of the thousand amounts
/// before it; every hundredth is the one before it divided by twelve.
fn recalc<N: Number>(values: &[N]) -> (Duration, String) {
    const CELLS: usize = 1_000_000;
    let mut ctx = N::ctx();
    let rate = N::parse(&mut ctx, "1.0001").unwrap();
    let twelve = N::parse(&mut ctx, "12").unwrap();
    let mut cells: Vec<N> = Vec::with_capacity(CELLS);
    let start = Instant::now();
    cells.push(values[0]);
    for i in 1..CELLS {
        let amount = values[i % values.len()];
        let cell = if i % 1000 == 0 {
            let mut sum = values[(i - 1000) % values.len()];
            for k in i - 999..i {
                sum = N::add(&mut ctx, sum, values[k % values.len()]);
            }
            sum
        } else if i % 100 == 0 {
            N::div(&mut ctx, cells[i - 1], twelve)
        } else {
            let grown = N::mul(&mut ctx, cells[i - 1], rate);
            N::add(&mut ctx, grown, amount)
        };
        cells.push(cell);
    }
    let elapsed = start.elapsed();
    (elapsed, black_box(cells[CELLS - 1]).text())
}

/// Nanoseconds per operation since `start`, over `count` operations.
fn per_op(start: Instant, count: usize) -> f64 {
    start.elapsed().as_secs_f64() * 1e9 / count as f64
}
