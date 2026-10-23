//! Markdown files as themselves (docs/ARCHITECTURE.md, section 6; reviews
//! frhewnk0, pysc65hp and ys9vyag1): a `.md` file opens as a structure-only
//! story, in the draft view, and every edit writes it back whole, through
//! a rename, as the `markdown` crate writes it: what wasn't edited byte
//! for byte as it was. Undo and named versions last as long as it's open.
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

use super::{Editor, EditorError, Effect, Panel, Pick, View};
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

/// Writes `bytes` as the file at `path`, whole and at once: beside it,
/// then renamed into its place, so it's never half written; with the
/// file's permissions, and through a symbolic link to the file it names.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut name = std::ffi::OsString::from(".");
    name.push(target.file_name().unwrap_or_default());
    name.push(".xworks-new");
    let new = target.with_file_name(name);
    std::fs::write(&new, bytes)?;
    if let Ok(meta) = std::fs::metadata(&target) {
        let _ = std::fs::set_permissions(&new, meta.permissions());
    }
    std::fs::rename(&new, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&new);
    })
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
            .map(|(link, p)| (link.clone(), placed_size(&res.page, p.width, p.height)))
            .collect();
        let language = language(&res);
        let settings = markdown::Settings {
            sheet: &res.sheet,
            language: language.as_deref(),
            sizes: &|link| sizes.get(link).copied(),
        };
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
        let lock = match store::acquire(path) {
            store::Acquired::Held(lock) => Some(lock),
            store::Acquired::Unavailable => None,
            store::Acquired::InUse(holder) => {
                read_only.get_or_insert_with(|| {
                    format!("open in {}", holder.as_deref().unwrap_or("another program"))
                });
                None
            }
        };
        let mut editor = Editor::with(
            res,
            path.to_path_buf(),
            None,
            read.document.clone(),
            History::new(),
        );
        editor.markdown = Some(Box::new(Markdown {
            file: read.file,
            lock,
            read_only,
            opened: read.document,
            log: Vec::new(),
            pending: None,
            version: false,
        }));
        for (link, decoded) in decoded {
            editor.pictures.insert(link, decoded.image);
        }
        editor.load_pictures(story::pictures(&editor.document));
        // A Markdown document opens as a draft (review frhewnk0).
        editor.view = View::Draft;
        editor.relayout(true);
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

    /// The Markdown file's lock, if it's one and holds it, to take along
    /// when it moves.
    pub(super) fn markdown_lock(&mut self) -> Option<&mut store::Lock> {
        self.markdown.as_mut().and_then(|m| m.lock.as_mut())
    }

    /// Writes the Markdown file as the document stands, after `logged`
    /// `t`: false, with the edit not written, if Markdown can't hold it.
    pub(super) fn save_markdown(
        &mut self,
        logged: Logged,
        t: &Transaction,
    ) -> Result<(), markdown::Unheld> {
        let Some(md) = &mut self.markdown else {
            return Ok(());
        };
        let bytes = md.file.write(&self.document, &self.res.sheet)?;
        md.log.push((logged, t.clone()));
        if let Err(e) = write_whole(&self.path, &bytes) {
            self.message = format!("Couldn't save: {e}");
        }
        Ok(())
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
        let mut editor = Editor::with(
            res,
            path.to_path_buf(),
            None,
            document.clone(),
            History::new(),
        );
        editor.markdown = Some(Box::new(Markdown {
            file,
            lock: None,
            read_only: Some("a version, opened to read".to_string()),
            opened: document,
            log: Vec::new(),
            pending: None,
            version: true,
        }));
        editor.load_pictures(story::pictures(&editor.document));
        editor.view = View::Draft;
        editor.relayout(true);
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
            .write(&self.document, &self.res.sheet)
            .map_err(|e| e.to_string())?;
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        write_whole(path, &bytes).map_err(|e| e.to_string())
    }

    /// The window closes: a Markdown document's versions go.
    pub(crate) fn close_markdown(&mut self) {
        if self.markdown.as_ref().is_some_and(|m| !m.version) {
            self.res
                .markdown_versions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&self.path);
        }
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
