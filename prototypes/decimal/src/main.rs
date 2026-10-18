//! The decimal prototype (docs/ROADMAP.md, M1 stage 2): measures candidates
//! for calc's 128-bit decimal number type against IEEE 754 decimal128's test
//! cases and against `f64`'s speed.
//!
//! ```text
//! ./fetch-dectest.sh                         fetch the test cases (once)
//! cargo run --release -- dectest [DIR]       run them (DIR: target/dectest)
//! cargo run --release -- bench               time the candidates
//! cargo run --release -- diff [N]            compare calc with libdecNumber
//! cargo run --release -- facts               small cases people notice
//! ```

mod bench;
mod candidates;
mod dectest;
mod diff;

use std::path::PathBuf;

use candidates::Number;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("dectest") => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| "target/dectest".into());
            run_dectest::<f64>(&dir);
            #[cfg(feature = "fastnum")]
            run_dectest::<fastnum::D128>(&dir);
            #[cfg(feature = "dec")]
            run_dectest::<dec::Decimal128>(&dir);
            #[cfg(feature = "calc")]
            run_dectest::<calc::decimal::Decimal>(&dir);
        }
        Some("bench") => {
            let texts = bench::amounts(100_000);
            let mut all = vec![bench::time::<f64>(&texts, 20)];
            #[cfg(feature = "fastnum")]
            all.push(bench::time::<fastnum::D128>(&texts, 20));
            #[cfg(feature = "dec")]
            all.push(bench::time::<dec::Decimal128>(&texts, 20));
            #[cfg(feature = "calc")]
            all.push(bench::time::<calc::decimal::Decimal>(&texts, 20));
            println!(
                "| Candidate | parse | add | multiply | divide | 1M-cell recalc | last cell |"
            );
            println!("|---|---|---|---|---|---|---|");
            for t in &all {
                println!(
                    "| {} | {:.1} ns | {:.1} ns | {:.1} ns | {:.1} ns | {:.1?} | {} |",
                    t.name, t.parse, t.add, t.mul, t.div, t.recalc, t.recalc_result
                );
            }
        }
        Some("diff") => {
            let count = args
                .get(1)
                .and_then(|n| n.parse().ok())
                .unwrap_or(1_000_000);
            diff::run(count);
        }
        Some("facts") => {
            facts::<f64>();
            #[cfg(feature = "fastnum")]
            facts::<fastnum::D128>();
            #[cfg(feature = "dec")]
            facts::<dec::Decimal128>();
            #[cfg(feature = "calc")]
            facts::<calc::decimal::Decimal>();
        }
        _ => eprintln!("usage: decimal-prototype dectest [DIR] | bench | diff [N] | facts"),
    }
}

fn run_dectest<N: Number>(dir: &std::path::Path) {
    println!("## {}\n", N::NAME);
    println!("| Operation | exact | by value | failed | skipped |");
    println!("|---|---|---|---|---|");
    let mut examples = Vec::new();
    for (op, file) in dectest::FILES {
        match dectest::run::<N>(op, &dir.join(file)) {
            Ok(t) => {
                println!(
                    "| {op} | {} | {} | {} | {} |",
                    t.exact, t.by_value, t.failed, t.skipped
                );
                examples.extend(t.examples);
            }
            Err(e) => println!("| {op} | {e} | | | |"),
        }
    }
    if !examples.is_empty() {
        println!("\nFirst failures:\n");
        for e in examples.iter().take(12) {
            println!("- `{e}`");
        }
    }
    println!();
}

fn facts<N: Number>() {
    let mut ctx = N::ctx();
    let num = |s: &str| N::parse(&mut N::ctx(), s).unwrap();
    let (tenth, fifth, one, three, cent, huge) = (
        num("0.1"),
        num("0.2"),
        num("1"),
        num("3"),
        num("0.01"),
        num("1E300"),
    );
    let mut cents = num("0");
    let sum = N::add(&mut ctx, tenth, fifth);
    let third = N::div(&mut ctx, one, three);
    let back = N::mul(&mut ctx, third, three);
    for _ in 0..10_000 {
        cents = N::add(&mut ctx, cents, cent);
    }
    let big = N::mul(&mut ctx, huge, huge);
    println!(
        "{:<16} 0.1+0.2 = {} | 1/3 = {} | (1/3)×3 = {} | 10000 × 0.01 = {} | 1E300×1E300 = {}",
        N::NAME,
        sum.text(),
        third.text(),
        back.text(),
        cents.text(),
        big.text()
    );
}
