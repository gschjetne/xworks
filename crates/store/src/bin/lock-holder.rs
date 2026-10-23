//! A helper for store's lock tests (tests/lock.rs), not a program to use:
//! opens the document at its first argument, making it first if there's
//! none, and prints "writable" or "read-only: WHY". If it can write, it
//! types its second argument at the end of the first paragraph, if
//! given, and prints "typed". Then it waits until its standard input
//! closes, or it's killed, and lets the document go.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use doc::{Document, Op, Props, Run};
use store::{Entry, Store};

fn main() -> Result<(), store::Error> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next().expect("a path"));
    let text = args.next().map(|t| t.to_string_lossy().into_owned());
    let (mut store, mut document) = if path.exists() {
        let opened = Store::open(&path)?;
        (opened.store, opened.document)
    } else {
        let document = Document::new();
        (Store::create(&path, &document, "story")?, document)
    };
    let mut out = std::io::stdout().lock();
    match store.read_only() {
        None => {
            let _ = writeln!(out, "writable");
            if let Some(text) = text {
                let paragraph = document.paragraphs()[0];
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
                store.append(Entry::Commit(&t), &document)?;
                let _ = writeln!(out, "typed");
            }
        }
        Some(why) => {
            let _ = writeln!(out, "read-only: {why}");
        }
    }
    let _ = out.flush();
    drop(out);
    let mut rest = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut rest);
    Ok(())
}
