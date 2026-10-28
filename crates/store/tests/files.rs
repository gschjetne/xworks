//! Files of other formats than xWorks' own, as Markdown files are (review
//! jy4a2tx5): written whole and durably, told apart when another program
//! changes them, and locked from xWorks' own folder, not beside them.

use std::path::PathBuf;

use store::{Acquired, acquire_at, lock_in, stamp, write_whole};

fn scratch(name: &str) -> PathBuf {
    testkit::scratch_dir(&format!("store-files-{name}")).expect("a scratch directory")
}

/// A file written whole is there whole, nothing left beside it, and its
/// stamp is what's on disk; another program's writing over it, or putting
/// another file of the same size in its place, changes its stamp.
#[test]
fn a_file_written_whole_and_its_stamp() {
    let dir = scratch("whole");
    let path = dir.join("Notes.md");
    std::fs::write(&path, b"old\n").expect("it writes");
    let written = write_whole(&path, b"new text\n").expect("it writes whole");
    assert_eq!(std::fs::read(&path).expect("it reads"), b"new text\n");
    assert_eq!(stamp(&path).as_ref(), Some(&written));
    let names: Vec<_> = std::fs::read_dir(&dir)
        .expect("the folder")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    assert_eq!(names, ["Notes.md"]);
    // Written over in place, longer.
    std::fs::write(&path, b"new text, theirs\n").expect("it writes");
    assert_ne!(stamp(&path).as_ref(), Some(&written));
    // Another file of the same size in its place.
    let written = write_whole(&path, b"same size\n").expect("it writes whole");
    let other = dir.join("other.md");
    std::fs::write(&other, b"SAME SIZE\n").expect("it writes");
    std::fs::rename(&other, &path).expect("it renames");
    let now = stamp(&path).expect("a stamp");
    assert_eq!(now.len, written.len);
    assert_ne!(now, written, "another file");
    assert_eq!(stamp(&dir.join("gone.md")), None);
}

/// Written through a symbolic link, the file it names is written, and the
/// link stays a link.
#[cfg(unix)]
#[test]
fn a_file_written_through_a_link() {
    let dir = scratch("link");
    let target = dir.join("target.md");
    std::fs::write(&target, b"one\n").expect("it writes");
    let link = dir.join("link.md");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&target, &link).expect("a link");
    write_whole(&link, b"two\n").expect("it writes whole");
    assert_eq!(std::fs::read(&target).expect("it reads"), b"two\n");
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the link")
            .file_type()
            .is_symlink()
    );
}

/// A Markdown file's lock is in xWorks' folder, named by its canonical
/// path: the same however the file is reached, another for another file;
/// the folder is made as it's first needed; and the lock moves when the
/// file does.
#[test]
fn a_lock_in_a_folder_of_locks() {
    let dir = scratch("locks");
    let folder = dir.join("locks");
    let _ = std::fs::remove_dir_all(&folder);
    let notes = dir.join("Notes.md");
    std::fs::write(&notes, b"notes\n").expect("it writes");
    let other = dir.join("Other.md");
    std::fs::write(&other, b"other\n").expect("it writes");
    let lock = lock_in(&folder, &notes);
    assert_eq!(lock.parent(), Some(folder.as_path()));
    assert_eq!(lock_in(&folder, &dir.join(".").join("Notes.md")), lock);
    assert_ne!(lock_in(&folder, &other), lock);
    let Acquired::Held(mut held) = acquire_at(&lock) else {
        panic!("the lock is had");
    };
    let Acquired::InUse(holder) = acquire_at(&lock) else {
        panic!("held twice");
    };
    // Where the system lets the lock file be read, it says who has it;
    // on Windows the lock keeps others from reading it.
    if cfg!(unix) {
        assert!(holder.is_some());
    }
    let moved = lock_in(&folder, &other);
    held.moved_to(&moved).expect("it moves");
    assert!(matches!(acquire_at(&moved), Acquired::InUse(_)));
    assert!(matches!(acquire_at(&lock), Acquired::Held(_)));
    drop(held);
    assert!(!moved.exists(), "its file goes with it");
}
