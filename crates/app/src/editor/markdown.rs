//! Markdown files as themselves (docs/ARCHITECTURE.md, section 6; reviews
//! frhewnk0, pysc65hp, ys9vyag1 and jy4a2tx5): a `.md` file opens as a
//! structure-only story, in the draft view, and every edit writes it back
//! whole, as the `markdown` crate writes it: what wasn't edited byte for
//! byte as it was. The edit makes the file's bytes, which checks Markdown
//! holds it; a worker writes them a moment later, gathered with the edits
//! after (`saver`). Undo and named versions last as long as it's open.
//!
//! Its lock lives in xWorks' own folder, not beside it, so git never sees
//! it (`store::lock_in`).
//!
//! A change another program makes to the file is noticed before xWorks
//! writes and when its window comes forward: with nothing of its own
//! unwritten, xWorks reads the file again, as an edit Undo takes back;
//! otherwise it asks whether to keep the file's version, its own, or its
//! own as a copy beside it.
//!
//! An edit Markdown can't hold, such as a font, isn't made until the
//! person says: the document becomes an `.xw` file beside it, with its
//! history and versions so far and the pictures it links to, and the edit
//! is made there; or it stays Markdown, without the edit. A Markdown file
//! never changes format unasked.
//!
//! Its pictures are linked where they are, by their paths, relative to
//! the file's folder where a relative path exists.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use doc::{Document, History, Object, Op, Props, Run, Snapshot, Transaction, Value};
use store::{Entry, Store};
use story::keys;

use super::saver::{self, Saver};
use super::{Editor, EditorError, Effect, Panel, Pick, Position, View};
use crate::Resources;
use crate::picture;

/// What an edit was, as an `.xw` file's log would record it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Logged {
    Commit,
    Continue,
    Undo,
    Redo,
}

/// An edit Markdown can't hold, waiting on the person's answer.
#[derive(Clone, Debug)]
pub(super) struct Pending {
    name: String,
    ops: Vec<Op>,
    what: markdown::What,
    /// Pictures the edit brings that an `.xw` file keeps, as a paste
    /// from one does.
    pictures: Vec<store::Image>,
}

/// A Markdown document's file, open.
pub(super) struct Markdown {
    /// What the file said, for writing it back.
    file: markdown::File,
    /// Its lock, while it's open to write, where locks can be had.
    lock: Option<store::Lock>,
    /// Why it can't be written, if it can't.
    read_only: Option<String>,
    /// The story as it opened, and every edit since, as an `.xw` file's
    /// log would hold them, for when it becomes one.
    opened: Document,
    log: Vec<(Logged, Transaction)>,
    /// An edit waiting on the person's answer.
    pending: Option<Pending>,
    /// Whether it's a named version shown, read-only.
    version: bool,
    /// What writes it, while it's open to write.
    saver: Option<Saver>,
    /// Each time it was read again, the file as it was before and after,
    /// by the first block the reading put in: undone, the file before is
    /// what's written from, so it's written as it was.
    reread: Vec<(doc::ObjectId, markdown::File, markdown::File)>,
    /// Whether the person is being asked about a change another program
    /// made, or said to ask later.
    asking: bool,
    deferred: bool,
}

/// Whether `path` is a Markdown file's, by its extension.
pub(crate) fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["md", "markdown"].iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// The file a picture's `link` in a Markdown file in `folder` names: a
/// path relative to the folder, an absolute one, or a `file:` URL; as it's
/// written, or with its `%`-escapes undone. None for other URLs.
pub(crate) fn resolve(folder: &Path, link: &str) -> Option<PathBuf> {
    let link = link.strip_prefix("file://").unwrap_or(link);
    if link.contains("://") || link.starts_with("data:") || link.is_empty() {
        return None;
    }
    let at = |l: &str| {
        let path = Path::new(l);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            folder.join(path)
        }
    };
    let plain = at(link);
    if plain.is_file() {
        return Some(plain);
    }
    let decoded = percent_decode(link)?;
    Some(at(&decoded))
}

/// `s` with its `%XX` escapes undone, if it has any and they're UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// How a Markdown file in `folder` links to the file at `path`: relative
/// to the folder where a relative path exists, with forward slashes; and
/// whether it had to be absolute, as on another drive on Windows.
pub(crate) fn link_to(folder: &Path, path: &Path) -> (String, bool) {
    let from: Vec<Component<'_>> = folder.components().collect();
    let to: Vec<Component<'_>> = path.components().collect();
    let same_root = from.first() == to.first() && folder.is_absolute() == path.is_absolute();
    if !same_root {
        return (path.to_string_lossy().replace('\\', "/"), true);
    }
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    (parts.join("/"), false)
}

/// Where the lock of the Markdown file at `path` is: in xWorks' own
/// folder of locks, or, where the shell gave none, the system's place
/// for temporary files.
pub(crate) fn lock_of(res: &Resources, path: &Path) -> PathBuf {
    let folder = res
        .locks
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("xworks-locks"));
    store::lock_in(&folder, path)
}

/// The name of the file at `path`, as a sentence says it.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The size a picture `width` by `height` pixels is placed at, on `page`:
/// at 96 pixels to the inch, scaled down to fit its body.
pub(super) fn placed_size(page: &canvas::PageStyle, width: u32, height: u32) -> (i64, i64) {
    use text::units::PIXEL;
    let (w, h) = (i64::from(width) * PIXEL, i64::from(height) * PIXEL);
    let (most_w, most_h) = (page.body_width().max(1), page.body_height().max(1));
    // The smaller of the two scales, as a fraction, in i128.
    let (num, den) = if w * most_h > h * most_w {
        (most_w, w)
    } else {
        (most_h, h)
    };
    if num >= den {
        return (w.max(1), h.max(1));
    }
    let scale = |side: i64| (i128::from(side) * i128::from(num) / i128::from(den)) as i64;
    (scale(w).max(1), scale(h).max(1))
}

/// The picture files `links` name in `folder`, decoded, by link: those
/// there are and that decode.
fn decode_links(
    res: &Resources,
    folder: &Path,
    links: &[String],
) -> Vec<(String, picture::Picture)> {
    use rayon::prelude::*;
    let limits = res.pictures;
    links
        .par_iter()
        .filter_map(|link| {
            let path = resolve(folder, link)?;
            let bytes = std::fs::read(path).ok()?;
            let decoded = picture::decode(&bytes, &limits).ok()?;
            Some((link.clone(), decoded))
        })
        .collect()
}

/// The language a Markdown file is read in where it doesn't say: the
/// system's.
fn language(res: &Resources) -> Option<String> {
    (res.language != text::Language::und()).then(|| res.language.to_string())
}

impl Editor {
    /// The Markdown document at `path`, in the draft view: written to as
    /// it's edited, unless another program has it open, or it isn't UTF-8,
    /// which it then opens read-only to say.
    pub(super) fn open_markdown(res: Arc<Resources>, path: &Path) -> Result<Editor, EditorError> {
        let bytes = std::fs::read(path).map_err(|source| EditorError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let decoded = decode_links(&res, &folder, &markdown::links(&bytes));
        let sizes: HashMap<String, (i64, i64)> = decoded
            .iter()
            .map(|(link, p)| {
                (
                    link.clone(),
                    placed_size(&res.program.page, p.width, p.height),
                )
            })
            .collect();
        let language = language(&res);
        // A Markdown file can't hold a stylesheet: it takes the program's.
        let sheet = res.stylesheets[0].objects(story::SHEET_AT);
        let settings = markdown::Settings {
            sheet: &sheet,
            language: language.as_deref(),
            sizes: &|link| sizes.get(link).copied(),
        };
        let stamp = store::stamp(path);
        let mut read_only = None;
        let read = match markdown::read(&bytes, &settings) {
            Ok(read) => read,
            Err(markdown::ReadError::NotUtf8(_)) => {
                read_only = Some("it isn't UTF-8 text".to_string());
                let text = String::from_utf8_lossy(&bytes).into_owned();
                markdown::read(text.as_bytes(), &settings)
                    .map_err(|e| store::Error::Damaged(e.to_string()))?
            }
            Err(e) => return Err(store::Error::Damaged(e.to_string()).into()),
        };
        let lock = match store::acquire_at(&lock_of(&res, path)) {
            store::Acquired::Held(lock) => Some(lock),
            store::Acquired::Unavailable => None,
            store::Acquired::InUse(holder) => {
                read_only.get_or_insert_with(|| {
                    format!("open in {}", holder.as_deref().unwrap_or("another program"))
                });
                None
            }
        };
        let after = std::time::Duration::from_millis(res.editing.markdown_saved_after);
        let saver = read_only.is_none().then(|| Saver::new(stamp, after));
        // A Markdown document opens as a draft (review frhewnk0), laid out
        // once, as one.
        let mut editor = Editor::with_view(
            res,
            path.to_path_buf(),
            None,
            read.document.clone(),
            History::new(),
            View::Draft,
        );
        editor.markdown = Some(Box::new(Markdown {
            file: read.file,
            lock,
            read_only,
            opened: read.document,
            log: Vec::new(),
            pending: None,
            version: false,
            saver,
            reread: Vec::new(),
            asking: false,
            deferred: false,
        }));
        for (link, decoded) in decoded {
            editor.pictures.insert(link, decoded.image);
        }
        editor.load_pictures(story::pictures(&editor.document));
        editor.unwritable();
        Ok(editor)
    }

    /// A new, empty Markdown file, "Untitled.md" (or "Untitled 2.md" and
    /// so on) in the documents folder, made at once, to open.
    pub(crate) fn new_markdown(res: &Resources) -> Result<PathBuf, EditorError> {
        std::fs::create_dir_all(&res.documents).map_err(|source| EditorError::Io {
            path: res.documents.clone(),
            source,
        })?;
        let path = super::file::free_path(&res.documents, "Untitled", Path::new(""), "md");
        std::fs::write(&path, b"").map_err(|source| EditorError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Whether the document is a Markdown file's, which saves as itself.
    pub fn is_markdown(&self) -> bool {
        self.markdown.is_some()
    }

    /// Why the Markdown file can't be written, if it's one and can't be.
    pub(super) fn markdown_read_only(&self) -> Option<Option<&str>> {
        self.markdown.as_ref().map(|m| m.read_only.as_deref())
    }

    /// Writes the Markdown file as the document stands, after `logged`
    /// `t`, or hands it to its saver to write in a moment: an error, with
    /// the edit not written, if Markdown can't hold it. Undoing or redoing
    /// a reading of the file again writes from the file as it was before
    /// or after it.
    pub(super) fn save_markdown(
        &mut self,
        logged: Logged,
        t: &Transaction,
    ) -> Result<(), markdown::Unheld> {
        let Some(md) = &mut self.markdown else {
            return Ok(());
        };
        // A Markdown file takes the program's stylesheet, as it can't hold
        // one: a change to it is asked about.
        let restyles = |op: &Op| {
            matches!(
                op,
                Op::InsertStyle { .. }
                    | Op::DeleteStyle { .. }
                    | Op::SetStyleProp { .. }
                    | Op::SetBase { .. }
            )
        };
        if t.ops.iter().any(restyles) {
            return Err(markdown::Unheld {
                block: self.document.sheet().unwrap_or(self.document.root()),
                what: markdown::What::Stylesheet,
            });
        }
        if let Some(doc::Op::InsertBlock { objects, .. }) = t.ops.first()
            && let Some((first, _)) = objects.first()
            && let Some((_, before, after)) = md.reread.iter().find(|(id, ..)| id == first)
        {
            match logged {
                Logged::Undo => md.file = before.clone(),
                Logged::Redo => md.file = after.clone(),
                Logged::Commit | Logged::Continue => {}
            }
        }
        let bytes = md.file.write(&self.document, &self.sheet)?;
        md.log.push((logged, t.clone()));
        md.deferred = false;
        if let Some(saver) = &md.saver {
            saver.put(&self.path, bytes);
        }
        Ok(())
    }

    /// Writes what's waiting to be written of the Markdown file at once,
    /// as closing does, saying so if it fails.
    pub(crate) fn flush_markdown(&mut self) {
        let Some(saver) = self.markdown.as_ref().and_then(|m| m.saver.as_ref()) else {
            return;
        };
        if let Err(e) = saver.flush() {
            self.message = format!("Couldn't save: {e}");
        }
    }

    /// What the Markdown file's saver has to say, said: a failure to
    /// write, the file written again after another program took it, or a
    /// change another program made, which it asks about. Whether there
    /// was anything.
    pub(crate) fn poll_markdown(&mut self) -> bool {
        let Some(md) = &self.markdown else {
            return false;
        };
        let Some(saver) = &md.saver else {
            return false;
        };
        let news = saver.news();
        let (asking, deferred) = (md.asking, md.deferred);
        let mut said = false;
        if let Some(e) = news.error {
            self.message = format!("Couldn't save: {e}");
            said = true;
        }
        if news.recreated {
            self.message = format!("{} was gone, and is written again", file_name(&self.path));
            said = true;
        }
        if news.conflict && !asking && !deferred && self.panel.is_none() {
            self.ask_about_disk();
            said = true;
        }
        said
    }

    /// The window came forward: a change another program made to the
    /// Markdown file is read, if xWorks has nothing of its own unwritten,
    /// or asked about.
    pub(crate) fn markdown_came_forward(&mut self) {
        let Some(md) = &mut self.markdown else {
            return;
        };
        md.deferred = false;
        let Some(saver) = &md.saver else {
            return;
        };
        let now = store::stamp(&self.path);
        if now == saver.stamp() {
            self.poll_markdown();
            return;
        }
        if now.is_none() {
            // Gone: what's unwritten is written again, and an edit writes
            // it again too.
            self.message = format!(
                "{} is gone; it's written again as you change it",
                file_name(&self.path)
            );
            return;
        }
        if saver.idle() {
            self.read_again();
        } else {
            saver.hold();
            self.ask_about_disk();
        }
    }

    /// Asks what to do about a change another program made to the
    /// Markdown file, with what xWorks has of its own unwritten: keep the
    /// file's version, and read it again; keep xWorks', and write it over
    /// the file; or keep xWorks' as a copy beside the file.
    fn ask_about_disk(&mut self) {
        let Some(md) = &mut self.markdown else {
            return;
        };
        md.asking = true;
        let name = file_name(&self.path);
        let copy = file_name(&saver::copy_path(&self.path));
        self.panel = Some(Panel {
            title: format!("Another program changed {name}"),
            rows: vec![
                (
                    format!("Keep its version, and read {name} again"),
                    Pick::Reread,
                ),
                (
                    format!("Keep yours, and write it over {name}"),
                    Pick::Overwrite,
                ),
                (format!("Keep yours as {copy}, beside it"), Pick::Copy),
            ],
            selected: 0,
        });
        self.message = format!("Another program changed {name}: keep its version, or yours?");
    }

    /// The question about a change another program made, closed without
    /// an answer: nothing's written, and it's asked again at the next edit
    /// or when the window comes forward.
    pub(super) fn defer_disk(&mut self) {
        if let Some(md) = &mut self.markdown
            && md.asking
        {
            md.asking = false;
            md.deferred = true;
            self.message = format!(
                "Not written: another program changed {}",
                file_name(&self.path)
            );
        }
    }

    /// Keeps xWorks' version of the Markdown file, written over the one
    /// another program changed.
    pub(super) fn overwrite_disk(&mut self) {
        let Some(md) = &mut self.markdown else {
            return;
        };
        md.asking = false;
        let Some(saver) = &md.saver else {
            return;
        };
        saver.take();
        saver.set_stamp(store::stamp(&self.path));
        if let Ok(bytes) = md.file.write(&self.document, &self.sheet) {
            saver.put(&self.path, bytes);
        }
        self.flush_markdown();
        if self.message.is_empty() || self.message.starts_with("Another program") {
            self.message = format!("Wrote yours over {}", file_name(&self.path));
        }
    }

    /// Keeps xWorks' version of the Markdown file as a copy beside the one
    /// another program changed, "NAME (xWorks).md", which the window then
    /// shows; the file stays as the other program left it.
    pub(super) fn copy_disk(&mut self) {
        let copy = saver::copy_path(&self.path);
        let locks = lock_of(&self.res, &copy);
        let Some(md) = &mut self.markdown else {
            return;
        };
        md.asking = false;
        let bytes = match md.file.write(&self.document, &self.sheet) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message = format!("Couldn't keep a copy: {e}");
                return;
            }
        };
        let stamp = match store::write_whole(&copy, &bytes) {
            Ok(stamp) => stamp,
            Err(e) => {
                self.message = format!("Couldn't keep a copy: {e}");
                return;
            }
        };
        if let Some(saver) = &md.saver {
            saver.take();
            saver.set_stamp(Some(stamp));
        }
        if let Some(lock) = &mut md.lock {
            let _ = lock.moved_to(&locks);
        }
        let from = std::mem::replace(&mut self.path, copy);
        self.markdown_moved(&from);
        self.res.recent.add(&self.path);
        self.named = true;
        self.message = format!(
            "Kept yours as {}; {} is as the other program left it",
            file_name(&self.path),
            file_name(&from)
        );
    }

    /// Reads the Markdown file again, as another program left it, as one
    /// edit Undo takes back: the story's blocks replaced by the file's.
    /// What xWorks had unwritten is dropped; undone, it's back, and
    /// written.
    pub(super) fn read_again(&mut self) {
        let name = file_name(&self.path);
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message = format!("Couldn't read {name} again: {e}");
                return;
            }
        };
        let stamp = store::stamp(&self.path);
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let decoded = decode_links(&self.res, &folder, &markdown::links(&bytes));
        let sizes: HashMap<String, (i64, i64)> = decoded
            .iter()
            .map(|(link, p)| {
                (
                    link.clone(),
                    placed_size(&self.res.program.page, p.width, p.height),
                )
            })
            .collect();
        let language = language(&self.res);
        // The program's stylesheet, as the file was read in: the story read
        // again goes into the document beside it.
        let sheet = self.res.stylesheets[0].objects(story::SHEET_AT);
        let settings = markdown::Settings {
            sheet: &sheet,
            language: language.as_deref(),
            sizes: &|link| sizes.get(link).copied(),
        };
        // Its objects get IDs the document hasn't used.
        let first = self.document.clone().new_id().0;
        let read = match markdown::read_from(&bytes, &settings, first) {
            Ok(read) => read,
            Err(e) => {
                self.message = format!("Couldn't read {name} again: {e}");
                return;
            }
        };
        let root = self.document.root();
        let old: Vec<doc::ObjectId> = self.document.children(root).to_vec();
        let mut after = old.last().copied();
        let mut ops = Vec::new();
        let new_root = read.document.root();
        for &block in read.document.children(new_root) {
            let objects: Vec<(doc::ObjectId, Object)> = read
                .document
                .subtree(block)
                .into_iter()
                .filter_map(|id| read.document.object(id).map(|o| (id, o.clone())))
                .collect();
            ops.push(Op::InsertBlock {
                parent: root,
                after,
                objects,
            });
            after = Some(block);
        }
        ops.extend(old.into_iter().map(|block| Op::DeleteBlock { block }));
        let Some(&first_block) = read.document.children(new_root).first() else {
            return;
        };
        let t = match self.document.transact("Read Again", ops) {
            Ok(t) => t,
            Err(e) => {
                self.message = format!("Couldn't read {name} again: {e}");
                return;
            }
        };
        let Some(md) = &mut self.markdown else {
            return;
        };
        let before = std::mem::replace(&mut md.file, read.file.clone());
        md.reread.push((first_block, before, read.file));
        md.log.push((Logged::Commit, t.clone()));
        md.asking = false;
        if let Some(saver) = &md.saver {
            saver.take();
            saver.set_stamp(stamp);
        }
        self.history.push(t);
        self.typing = None;
        for (link, decoded) in decoded {
            self.pictures.insert(link, decoded.image);
        }
        self.load_pictures(story::pictures(&self.document));
        self.relayout(true);
        let caret = match self.document.paragraph(self.focus.paragraph) {
            Some(p) => Position {
                offset: self.focus.offset.min(p.len()),
                ..self.focus
            },
            None => self.document_edge(false),
        };
        self.go(caret, false);
        self.message = format!("Read {name} again, as another program left it");
    }

    /// Asks before an edit Markdown can't hold, `ops` named `name`, which
    /// isn't made: it can make the document an `.xw` file, or leave it
    /// Markdown without it.
    pub(super) fn ask_to_convert(&mut self, name: &str, ops: Vec<Op>, unheld: markdown::Unheld) {
        let Some(md) = &mut self.markdown else {
            return;
        };
        md.pending = Some(Pending {
            name: name.to_string(),
            ops,
            what: unheld.what.clone(),
            pictures: std::mem::take(&mut self.pasted_pictures),
        });
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let to = super::file::free_path(&folder, &self.stem(), Path::new(""), "xw");
        let xw = to
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.panel = Some(Panel {
            title: format!("Markdown can't hold {}", unheld.what),
            rows: vec![
                (format!("Make it an xWorks document, {xw}"), Pick::Convert),
                (
                    "Keep it Markdown, without the change".to_string(),
                    Pick::Keep,
                ),
            ],
            selected: 0,
        });
        self.message = format!(
            "Markdown can't hold {}: make it an xWorks document?",
            unheld.what
        );
    }

    /// Leaves the document Markdown, without the edit it couldn't hold.
    pub(super) fn keep_markdown(&mut self) {
        if let Some(md) = &mut self.markdown
            && let Some(pending) = md.pending.take()
        {
            self.message = format!(
                "Kept it Markdown, without the {}",
                pending.name.to_lowercase()
            );
        }
    }

    /// Makes the Markdown document an `.xw` file beside it, with its
    /// history, its named versions and the pictures it links to, which the
    /// file keeps; then makes the edit Markdown couldn't hold there. The
    /// Markdown file stays as it was last written.
    pub(super) fn convert(&mut self) -> Vec<Effect> {
        let Some(mut md) = self.markdown.take() else {
            return Vec::new();
        };
        let Some(pending) = md.pending.take() else {
            self.markdown = Some(md);
            return Vec::new();
        };
        match self.make_xw(&md, &pending) {
            Ok((path, store, map)) => {
                let from = std::mem::replace(&mut self.path, path);
                self.store = Some(store);
                // The rest, as the file now has it: pictures by their
                // hashes.
                self.document = rehome_document(&self.document, &map);
                let (undo, redo) = self.history.stacks();
                let undo: Vec<Transaction> =
                    undo.iter().map(|t| rehome_transaction(t, &map)).collect();
                let redo: Vec<Transaction> =
                    redo.iter().map(|t| rehome_transaction(t, &map)).collect();
                self.history = History::from_stacks(undo, redo);
                for (link, id) in &map {
                    if let Some(image) = self.pictures.get(link).cloned() {
                        self.pictures.insert(id.clone(), image);
                    }
                }
                drop(md);
                self.res
                    .markdown_versions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&from);
                self.res.recent.add(&self.path);
                let ops = pending.ops.iter().map(|op| rehome_op(op, &map)).collect();
                self.relayout(true);
                self.commit(&pending.name, ops, false);
                let name = |p: &Path| {
                    p.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                };
                self.message = format!(
                    "Now {}, as Markdown can't hold {}; {} is as it was",
                    name(&self.path),
                    pending.what,
                    name(&from)
                );
            }
            Err(e) => {
                self.message = format!("Couldn't make it an xWorks document: {e}");
                self.markdown = Some(md);
            }
        }
        Vec::new()
    }

    /// The `.xw` file the Markdown document becomes: made beside it, with
    /// the story as it opened, every edit since and the versions named,
    /// and the pictures it links to, kept by their hashes. Its path, the
    /// file open, and which links became which pictures.
    fn make_xw(
        &self,
        md: &Markdown,
        pending: &Pending,
    ) -> Result<(PathBuf, Store, HashMap<String, String>), EditorError> {
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let to = super::file::free_path(&folder, &self.stem(), Path::new(""), "xw");
        // Every picture linked, in the story as it opened, as it is, and
        // in its history.
        let mut links: HashSet<String> = story::pictures(&md.opened).into_iter().collect();
        links.extend(story::pictures(&self.document));
        for (_, t) in &md.log {
            links.extend(Editor::pictures_in(&t.ops));
            links.extend(Editor::pictures_in(&t.inverse));
        }
        links.extend(Editor::pictures_in(&pending.ops));
        let mut kept: Vec<(String, &'static str, Vec<u8>)> = Vec::new();
        for link in links {
            let Some(path) = resolve(&folder, &link) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let Some(format) = picture::format(&bytes) else {
                continue;
            };
            kept.push((link, super::pictures::format_name(format), bytes));
        }
        let map: HashMap<String, String> = kept
            .iter()
            .map(|(link, _, bytes)| (link.clone(), store::image_id(bytes)))
            .collect();
        let opened = rehome_document(&md.opened, &map);
        let mut store = Store::create(&to, &opened, "story")?;
        for (_, format, bytes) in &kept {
            store.add_image(format, bytes)?;
        }
        for image in &pending.pictures {
            store.add_image(&image.format, &image.data)?;
        }
        // The log replayed, each entry after the story it made.
        let versions: Vec<(store::Version, Document)> = self
            .res
            .markdown_versions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.path)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let mut document = opened;
        for (n, (logged, t)) in md.log.iter().enumerate() {
            let t = rehome_transaction(t, &map);
            let ops = match logged {
                Logged::Undo => &t.inverse,
                _ => &t.ops,
            };
            document
                .apply_all(ops)
                .map_err(|e| store::Error::Damaged(e.to_string()))?;
            let entry = match logged {
                Logged::Commit => Entry::Commit(&t),
                Logged::Continue => Entry::Continue(&t),
                Logged::Undo => Entry::Undo(&t),
                Logged::Redo => Entry::Redo(&t),
            };
            store.append(entry, &document)?;
            for (version, _) in versions.iter().filter(|(v, _)| v.seq == n as i64 + 1) {
                store.name_version(&version.name)?;
            }
        }
        Ok((to, store, map))
    }

    /// Names the Markdown document as it is, as a version kept while it's
    /// open.
    pub(super) fn name_markdown_version(&mut self) {
        let Some(md) = &self.markdown else {
            return;
        };
        let mut all = self
            .res
            .markdown_versions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_, versions) = all
            .entry(self.path.clone())
            .or_insert_with(|| (md.file.clone(), Vec::new()));
        let name = format!("Version {}", versions.len() + 1);
        versions.push((
            store::Version {
                name: name.clone(),
                seq: md.log.len() as i64,
            },
            self.document.clone(),
        ));
        self.message = format!("Saved {name}, kept while it's open");
    }

    /// The Markdown document's named versions, oldest first.
    pub(super) fn markdown_versions(&self) -> Vec<store::Version> {
        self.res
            .markdown_versions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.path)
            .map(|(_, v)| v.iter().map(|(v, _)| v.clone()).collect())
            .unwrap_or_default()
    }

    /// The named version of the Markdown document at `path` that follows
    /// `seq` of its edits, read-only, as it was, if it's open.
    pub(super) fn open_markdown_version(
        res: Arc<Resources>,
        path: &Path,
        seq: i64,
    ) -> Result<Editor, EditorError> {
        let found = res
            .markdown_versions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .and_then(|(file, versions)| {
                versions
                    .iter()
                    .rev()
                    .find(|(v, _)| v.seq == seq)
                    .map(|(v, d)| (file.clone(), v.clone(), d.clone()))
            });
        let Some((file, version, document)) = found else {
            return Err(store::Error::Damaged(format!(
                "there's no version named after edit {seq} of {}",
                path.display()
            ))
            .into());
        };
        let mut editor = Editor::with_view(
            res,
            path.to_path_buf(),
            None,
            document.clone(),
            History::new(),
            View::Draft,
        );
        editor.markdown = Some(Box::new(Markdown {
            file,
            lock: None,
            read_only: Some("a version, opened to read".to_string()),
            opened: document,
            log: Vec::new(),
            pending: None,
            version: true,
            saver: None,
            reread: Vec::new(),
            asking: false,
            deferred: false,
        }));
        editor.load_pictures(story::pictures(&editor.document));
        editor.message = format!("{}, read-only: Copy Out to change it", version.name);
        editor.version = Some(version);
        Ok(editor)
    }

    /// Writes the version this window shows of a Markdown document as a
    /// Markdown file of its own, at `path`.
    pub(super) fn copy_out_markdown(&mut self, path: &Path) -> Result<(), String> {
        let Some(md) = &mut self.markdown else {
            return Err("it isn't Markdown".to_string());
        };
        let bytes = md
            .file
            .write(&self.document, &self.sheet)
            .map_err(|e| e.to_string())?;
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        store::write_whole(path, &bytes)
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// The window closes: what's unwritten of a Markdown document is
    /// written at once, or as a copy beside it if another program changed
    /// it and the person hasn't said; and its versions go.
    pub(crate) fn close_markdown(&mut self) {
        if self
            .markdown
            .as_ref()
            .and_then(|m| m.saver.as_ref())
            .is_some_and(Saver::conflict)
        {
            self.copy_disk();
        }
        self.flush_markdown();
        if self.markdown.as_ref().is_some_and(|m| !m.version) {
            self.res
                .markdown_versions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.path);
        }
    }

    /// The Markdown file moves from `from` to where the document now is:
    /// what's waiting is written first, where it was, and its lock and
    /// versions go along. Whether it may move: not while what's waiting
    /// can't be written.
    pub(super) fn markdown_moving(&mut self, from: &Path) -> Result<(), String> {
        let Some(saver) = self.markdown.as_ref().and_then(|m| m.saver.as_ref()) else {
            return Ok(());
        };
        let _ = from;
        if saver.conflict() {
            return Err(
                "another program changed it, and xWorks hasn't written what's yours".into(),
            );
        }
        saver.flush()
    }

    /// The Markdown file moved, from `from` to where the document now is:
    /// its lock, what it is on disk, and its versions follow it.
    pub(super) fn markdown_relocated(&mut self, from: &Path) {
        let lock = lock_of(&self.res, &self.path);
        if let Some(md) = &mut self.markdown {
            if let Some(held) = &mut md.lock {
                let _ = held.moved_to(&lock);
            }
            if let Some(saver) = &md.saver {
                saver.set_stamp(store::stamp(&self.path));
            }
        }
        self.markdown_moved(from);
    }

    /// The Markdown document's versions follow it from `from`, where it
    /// was, as it's renamed or moved.
    pub(super) fn markdown_moved(&mut self, from: &Path) {
        let mut all = self
            .res
            .markdown_versions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(versions) = all.remove(from) {
            all.insert(self.path.clone(), versions);
        }
    }
}

/// `props` with a picture's link made the ID `map` gives it.
fn rehome_props(props: &Props, map: &HashMap<String, String>) -> Option<Props> {
    let Some(Value::Text(link)) = props.get(keys::PICTURE) else {
        return None;
    };
    let id = map.get(link)?;
    let mut props = props.clone();
    props.insert(keys::PICTURE.to_string(), Value::Text(id.clone()));
    Some(props)
}

fn rehome_runs(runs: &[Run], map: &HashMap<String, String>) -> Vec<Run> {
    runs.iter()
        .map(|r| match rehome_props(&r.props, map) {
            Some(props) => Run::new(r.len, props),
            None => r.clone(),
        })
        .collect()
}

fn rehome_object(object: &Object, map: &HashMap<String, String>) -> Object {
    match object {
        Object::Paragraph(p) => {
            let mut p = p.clone();
            p.runs = rehome_runs(&p.runs, map);
            Object::Paragraph(p)
        }
        other => other.clone(),
    }
}

/// `document` with its pictures' links made the IDs `map` gives them.
fn rehome_document(document: &Document, map: &HashMap<String, String>) -> Document {
    if map.is_empty() {
        return document.clone();
    }
    let snapshot = document.to_snapshot();
    let objects = snapshot
        .objects
        .iter()
        .map(|(id, o)| (*id, rehome_object(o, map)))
        .collect();
    Document::from_snapshot(Snapshot {
        objects,
        ..snapshot
    })
    .unwrap_or_else(|_| document.clone())
}

/// `op` with its pictures' links made the IDs `map` gives them.
fn rehome_op(op: &Op, map: &HashMap<String, String>) -> Op {
    match op {
        Op::InsertText {
            paragraph,
            offset,
            text,
            runs,
        } => Op::InsertText {
            paragraph: *paragraph,
            offset: *offset,
            text: text.clone(),
            runs: rehome_runs(runs, map),
        },
        Op::InsertBlock {
            parent,
            after,
            objects,
        } => Op::InsertBlock {
            parent: *parent,
            after: *after,
            objects: objects
                .iter()
                .map(|(id, o)| (*id, rehome_object(o, map)))
                .collect(),
        },
        Op::SetCharProp {
            paragraph,
            offset,
            len,
            key,
            value: Some(Value::Text(link)),
        } if key == keys::PICTURE && map.contains_key(link) => Op::SetCharProp {
            paragraph: *paragraph,
            offset: *offset,
            len: *len,
            key: key.clone(),
            value: Some(Value::Text(map[link].clone())),
        },
        other => other.clone(),
    }
}

fn rehome_transaction(t: &Transaction, map: &HashMap<String, String>) -> Transaction {
    Transaction {
        name: t.name.clone(),
        ops: t.ops.iter().map(|op| rehome_op(op, map)).collect(),
        inverse: t.inverse.iter().map(|op| rehome_op(op, map)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_relative_where_they_can_be() {
        let folder = Path::new("/home/me/notes");
        assert_eq!(
            link_to(folder, Path::new("/home/me/notes/images/cat.png")),
            ("images/cat.png".to_string(), false)
        );
        assert_eq!(
            link_to(folder, Path::new("/home/me/Pictures/cat.png")),
            ("../Pictures/cat.png".to_string(), false)
        );
        assert_eq!(resolve(folder, "https://example.com/a.png"), None);
        assert_eq!(
            percent_decode("my%20cat.png").as_deref(),
            Some("my cat.png")
        );
    }
}
