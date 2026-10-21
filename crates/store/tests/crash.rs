//! A crash loses nothing committed (docs/ROADMAP.md, M1 stage 8): a writer
//! creating a document and appending to it is killed at a random moment, a
//! thousand times (XWORKS_CRASH_RUNS to change). Each time, the file either
//! isn't there yet, because the writer died before creating it, or opens at
//! a committed transaction, with every transaction the writer saw committed.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::Duration;

use store::Store;

/// The document after `n` of the writer's transactions: "1;2;…;n;".
fn expected(n: i64) -> String {
    (1..=n).map(|i| format!("{i};")).collect()
}

#[test]
fn a_crash_loses_nothing_committed() {
    let runs: u64 = std::env::var("XWORKS_CRASH_RUNS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1000);
    let dir = testkit::scratch_dir("store-crash").expect("a scratch directory");
    // A small generator, so the moments are the same every time.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut random = move |below: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % below
    };
    let (mut before_created, mut committed_runs, mut finished) = (0, 0, 0);
    for run in 0..runs {
        let path = dir.join(format!("run-{run}.xw"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_crash-writer"))
            .arg(&path)
            .stdout(Stdio::piped())
            .spawn()
            .expect("the writer starts");
        std::thread::sleep(Duration::from_micros(random(50_000)));
        let _ = child.kill();
        child.wait().expect("the writer stops");
        let mut out = String::new();
        child
            .stdout
            .take()
            .expect("its output")
            .read_to_string(&mut out)
            .expect("its output reads");
        let lines: Vec<&str> = out.lines().collect();
        let created = lines.first() == Some(&"created");
        let seen: i64 = lines.last().and_then(|l| l.parse().ok()).unwrap_or(0);

        if !path.exists() {
            assert!(
                !created,
                "run {run}: the writer created the file, but it isn't there"
            );
            before_created += 1;
            continue;
        }
        let opened =
            Store::open(&path).unwrap_or_else(|e| panic!("run {run}: the file doesn't open: {e}"));
        let at = opened.store.seq();
        assert!(
            at == seen || (at == seen + 1 && lines.len() as i64 == seen + 1),
            "run {run}: the file is at transaction {at}, but the writer saw {seen} committed"
        );
        let paragraph = opened.document.paragraphs()[0];
        assert_eq!(
            opened
                .document
                .paragraph(paragraph)
                .expect("a paragraph")
                .text,
            expected(at),
            "run {run}"
        );
        assert_eq!(
            opened.history.undo_name(),
            (at > 0).then_some("Typing"),
            "run {run}"
        );
        if at > 0 {
            committed_runs += 1;
        }
        if at == 250 {
            finished += 1;
        }
        drop(opened);
        std::fs::remove_file(&path).expect("the file goes");
    }
    // The moments spread over the writer's life: some before it made the
    // file, most while it was appending.
    assert!(
        runs < 100 || (before_created > 0 && committed_runs > runs / 4),
        "{before_created} runs ended before the file, {committed_runs} with transactions, {finished} finished"
    );
}
