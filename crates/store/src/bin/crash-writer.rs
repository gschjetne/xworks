//! A helper for store's crash test (tests/crash.rs), not a program to use:
//! creates the document at its argument, prints "created", then appends
//! transactions as fast as it can, printing each one's sequence number once
//! it's committed, until it's killed or has made 250.
//!
//! Transaction n types "n;" at the end of the first paragraph, so the
//! document after n transactions is known.

use std::io::Write as _;
use std::path::PathBuf;

use doc::{Document, Op, Props, Run};
use store::{Entry, Store};

fn main() -> Result<(), store::Error> {
    let path = PathBuf::from(std::env::args_os().nth(1).expect("a path"));
    let mut document = Document::new();
    let mut store = Store::create(&path, &document, "story")?;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "created");
    let _ = out.flush();
    let paragraph = document.paragraphs()[0];
    for n in 1..=250 {
        let text = format!("{n};");
        let offset = document.paragraph(paragraph).map_or(0, |p| p.len());
        let len = text.chars().count() as u32;
        let t = document
            .transact(
                "Typing",
                vec![Op::InsertText {
                    paragraph,
                    offset,
                    text,
                    runs: vec![Run::new(len, Props::new())],
                }],
            )
            .expect("typing applies");
        let seq = store.append(Entry::Commit(&t), &document)?;
        let _ = writeln!(out, "{seq}");
        let _ = out.flush();
    }
    Ok(())
}
