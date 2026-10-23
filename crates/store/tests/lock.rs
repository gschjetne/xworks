//! A document is never open twice (project owner, 2026-10-23, review
//! a23vvez8): the program that has it open to write holds its lock, and
//! any other opens it read-only and says why. Two processes, each the
//! lock-holder helper (src/bin/lock-holder.rs), and one process opening a
//! document twice, as two windows would.

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

use doc::{Document, Op, Props, Run};
use store::{Entry, Error, ReadOnly, Store};

fn scratch(name: &str) -> PathBuf {
    testkit::scratch_dir(&format!("store-lock-{name}")).expect("a scratch directory")
}

/// The helper, holding the document at `path`, typing `text` if it can
/// write, and the first line it says.
struct Holder {
    child: Child,
    out: BufReader<ChildStdout>,
}

impl Holder {
    fn start(path: &Path, text: Option<&str>) -> (Holder, String) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lock-holder"));
        command.arg(path);
        if let Some(text) = text {
            command.arg(text);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the helper starts");
        let out = BufReader::new(child.stdout.take().expect("its output"));
        let mut holder = Holder { child, out };
        let line = holder.line();
        (holder, line)
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.out.read_line(&mut line).expect("its output reads");
        line.trim_end().to_string()
    }

    /// Closes its standard input, so it lets the document go and ends.
    fn finish(mut self) {
        drop(self.child.stdin.take());
        let status = self.child.wait().expect("the helper ends");
        assert!(status.success(), "{status}");
    }

    /// Kills it, as a crash would end it.
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Types `text` at the end of the first paragraph.
fn typing(document: &mut Document, text: &str) -> doc::Transaction {
    let paragraph = document.paragraphs()[0];
    let offset = document.paragraph(paragraph).expect("a paragraph").len();
    document
        .transact(
            "Typing",
            vec![Op::InsertText {
                paragraph,
                offset,
                text: text.to_string(),
                runs: vec![Run::new(text.chars().count() as u32, Props::new())],
            }],
        )
        .expect("typing applies")
}

fn first_text(document: &Document) -> String {
    let p = document.paragraphs()[0];
    document.paragraph(p).expect("a paragraph").text.clone()
}

#[test]
fn another_program_has_it_so_it_opens_read_only_and_says_why() {
    let dir = scratch("two");
    let path = dir.join("shared.xw");
    let (first, said) = Holder::start(&path, Some("Written by the first."));
    assert_eq!(said, "writable");
    let mut first = first;
    assert_eq!(first.line(), "typed");
    let pid = first.child.id();

    // This process opens it read-only, and reads what the first wrote.
    let mut opened = Store::open(&path).expect("it opens");
    let Some(ReadOnly::InUse(holder)) = opened.store.read_only().cloned() else {
        panic!(
            "open to write in two programs: {:?}",
            opened.store.read_only()
        );
    };
    // Where the system lets the lock file be read, it says who has it.
    if cfg!(unix) {
        let holder = holder.expect("the holder says who it is");
        assert!(
            holder.starts_with(&format!("xWorks, process {pid}")),
            "{holder}"
        );
    }
    assert_eq!(first_text(&opened.document), "Written by the first.");
    let t = typing(&mut opened.document, " Not by this one.");
    assert!(matches!(
        opened.store.append(Entry::Commit(&t), &opened.document),
        Err(Error::InUse { .. })
    ));
    assert!(matches!(
        opened.store.name_version("No"),
        Err(Error::InUse { .. })
    ));
    assert!(matches!(
        opened.store.add_image("png", b"\x89PNG"),
        Err(Error::InUse { .. })
    ));
    drop(opened);

    // A third program is refused too.
    let (third, said) = Holder::start(&path, None);
    assert!(said.starts_with("read-only: open in "), "{said}");
    third.finish();

    // Once the first lets it go, its lock file goes, and the document
    // opens to write.
    first.finish();
    assert!(!store::lock_path(&path).exists());
    let opened = Store::open(&path).expect("it opens");
    assert_eq!(opened.store.read_only(), None);
    assert_eq!(first_text(&opened.document), "Written by the first.");
}

#[test]
fn a_lock_goes_with_its_program_however_it_ends() {
    let dir = scratch("crash");
    let path = dir.join("crashed.xw");
    let (holder, said) = Holder::start(&path, Some("Before the crash."));
    assert_eq!(said, "writable");
    holder.kill();
    // The lock file is left behind, but nothing holds it.
    let opened = Store::open(&path).expect("it opens");
    assert_eq!(opened.store.read_only(), None);
    assert_eq!(
        opened.store.lock_file(),
        Some(store::lock_path(&path).as_path())
    );
}

#[test]
fn one_program_opens_a_document_to_write_once() {
    let dir = scratch("once");
    let path = dir.join("once.xw");
    let mut document = Document::new();
    let mut created = Store::create(&path, &document, "story").expect("created");
    assert!(store::lock_path(&path).exists());
    let t = typing(&mut document, "Hello");
    created
        .append(Entry::Commit(&t), &document)
        .expect("appended");
    // Opened again, as a second window would: read-only.
    let second = Store::open(&path).expect("it opens");
    assert!(matches!(second.store.read_only(), Some(ReadOnly::InUse(_))));
    assert_eq!(first_text(&second.document), "Hello");
    drop(second);
    // Opening it only to read takes no lock and reads what's committed.
    let read = Store::open_to_read(&path).expect("it opens to read");
    assert_eq!(read.store.read_only(), Some(&ReadOnly::Asked));
    assert_eq!(first_text(&read.document), "Hello");
    drop(created);
    assert!(!store::lock_path(&path).exists());
    // With only a reader there, it opens to write.
    let writer = Store::open(&path).expect("it opens");
    assert_eq!(writer.store.read_only(), None);
    drop(read);
    drop(writer);
    // And a reader first takes nothing a writer needs.
    let read = Store::open_to_read(&path).expect("it opens to read");
    assert!(!store::lock_path(&path).exists());
    let writer = Store::open(&path).expect("it opens");
    assert_eq!(writer.store.read_only(), None);
    drop(read);
}

#[test]
fn a_moved_document_takes_its_lock_along() {
    let dir = scratch("moved");
    let path = dir.join("before.xw");
    let moved = dir.join("after.xw");
    let document = Document::new();
    let mut held = Store::create(&path, &document, "story").expect("created");
    // The document's own files are renamed by whatever moves it; here a
    // copy stands where it went, and the lock follows it there.
    held.copy_out(&moved, &document, &[]).expect("copied");
    held.moved(&moved).expect("the lock moves");
    assert!(!store::lock_path(&path).exists());
    assert!(store::lock_path(&moved).exists());
    assert_eq!(held.lock_file(), Some(store::lock_path(&moved).as_path()));
    let there = Store::open(&moved).expect("it opens");
    assert!(matches!(there.store.read_only(), Some(ReadOnly::InUse(_))));
    drop(there);
    drop(held);
    assert!(!store::lock_path(&moved).exists());
    let there = Store::open(&moved).expect("it opens");
    assert_eq!(there.store.read_only(), None);
}
