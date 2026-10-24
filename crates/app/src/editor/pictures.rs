//! Pictures placed in the text: Insert Picture asks the shell for a file
//! ([`Effect::Choose`]), and the file chosen is read, decoded, kept in the
//! document's file once by its hash, and placed at the caret as a
//! character whose run names it (`story::keys::PICTURE`). The pictures a
//! document shows are decoded when it opens, and when an edit, undo or
//! redo brings back one not decoded yet.
//!
//! A Markdown file keeps none: a picture placed in one is linked where it
//! is, by its path, relative to the file's folder where a relative path
//! reaches it (review ys9vyag1), and its ID is its link; pictures a file
//! links to are drawn from where they point.

use std::path::Path;

use doc::{Op, Props, Run, Value};
use rayon::prelude::*;
use story::{OBJECT, keys};

use super::{Choice, Editor, Position};
use crate::picture::{self, Format};

impl Editor {
    /// The file a person chose for `choice`, if they chose one.
    pub(super) fn chosen(&mut self, choice: Choice, path: Option<&Path>) {
        // Opening and moving documents through the system's dialogs are
        // M2's stage 5.
        if let (Choice::Picture, Some(path)) = (choice, path) {
            self.place_picture(path);
        }
    }

    /// Places the picture in the file at `path` at the caret, in place of
    /// the selection: kept in the document's file, then placed by an edit
    /// that names it, at its own size at 96 pixels to the inch, scaled
    /// down to fit the page's body, keeping its proportions. Says why if it
    /// can't be.
    fn place_picture(&mut self, path: &Path) {
        if self.unwritable() {
            return;
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message = format!("Couldn't place {name}: {e}");
                return;
            }
        };
        let decoded = match picture::decode(&bytes, &self.res.pictures) {
            Ok(decoded) => decoded,
            Err(e) => {
                self.message = format!("Couldn't place {name}: {e}");
                return;
            }
        };
        // In a Markdown file, linked; in an .xw file, kept, by its hash.
        let mut linked = None;
        let id = if self.is_markdown() {
            let folder = self
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            let (link, absolute) = super::markdown::link_to(&folder, &full);
            linked = Some(if absolute {
                "by its full path, as no relative one reaches it".to_string()
            } else {
                format!("as {link}")
            });
            link
        } else {
            let added = match &mut self.store {
                Some(store) => store.add_image(format_name(decoded.format), &bytes),
                None => return,
            };
            match added {
                Ok(id) => id,
                Err(e) => {
                    self.message = format!("Couldn't place {name}: {e}");
                    return;
                }
            }
        };
        self.pictures.insert(id.clone(), decoded.image);
        let (width, height) = self.placed_size(decoded.width, decoded.height);
        let alt = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let mut props = Props::new();
        if linked.is_some() {
            props.insert(keys::PICTURE_SRC.to_string(), Value::Text(id.clone()));
        }
        props.insert(keys::PICTURE.to_string(), Value::Text(id));
        props.insert(keys::PICTURE_ALT.to_string(), Value::Text(alt));
        props.insert(keys::PICTURE_WIDTH.to_string(), Value::Int(width));
        props.insert(keys::PICTURE_HEIGHT.to_string(), Value::Int(height));
        let (mut ops, at) = self.deletion();
        ops.push(Op::InsertText {
            paragraph: at.paragraph,
            offset: at.offset,
            text: OBJECT.to_string(),
            runs: vec![Run::new(1, props)],
        });
        if self.commit("Insert Picture", ops, false) {
            let after = Position {
                paragraph: at.paragraph,
                offset: at.offset + 1,
                upstream: true,
            };
            self.go(after, false);
            if let Some(linked) = linked {
                self.message = format!("Placed {name}, linked {linked}");
            }
        }
    }

    /// The size a picture `width` by `height` pixels is placed at: at 96
    /// pixels to the inch, scaled down to fit the page's body.
    fn placed_size(&self, width: u32, height: u32) -> (i64, i64) {
        super::markdown::placed_size(&self.sheet.page, width, height)
    }

    /// Decodes the pictures `ids` names that aren't decoded yet, from the
    /// document's file, on the worker pool. One the file lacks, or that
    /// doesn't decode, is drawn as a tint, and said so.
    pub(super) fn load_pictures(&mut self, ids: Vec<String>) {
        let wanted: Vec<String> = ids
            .into_iter()
            .filter(|id| !self.pictures.contains_key(id))
            .collect();
        if wanted.is_empty() {
            return;
        }
        // From the document's file, or where a Markdown file's links
        // point.
        let folder = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let markdown = self.is_markdown();
        let stored: Vec<(String, Option<store::Image>)> = wanted
            .into_iter()
            .map(|id| {
                let image = if markdown {
                    super::markdown::resolve(&folder, &id)
                        .and_then(|path| std::fs::read(path).ok())
                        .and_then(|data| {
                            let format = format_name(picture::format(&data)?).to_string();
                            Some(store::Image {
                                id: id.clone(),
                                format,
                                data,
                            })
                        })
                } else {
                    self.store
                        .as_ref()
                        .and_then(|s| s.image(&id).ok().flatten())
                };
                (id, image)
            })
            .collect();
        let limits = self.res.pictures;
        let decoded: Vec<(String, Option<scene::Image>)> = stored
            .into_par_iter()
            .map(|(id, image)| {
                let image = image.and_then(|image| {
                    let format = format_of(&image.format)?;
                    picture::decode_as(format, &image.data, &limits)
                        .ok()
                        .map(|p| p.image)
                });
                (id, image)
            })
            .collect();
        let mut missing = 0;
        for (id, image) in decoded {
            match image {
                Some(image) => {
                    self.pictures.insert(id, image);
                }
                None => missing += 1,
            }
        }
        if missing > 0 {
            self.message = match missing {
                1 => "A picture couldn't be shown".to_string(),
                n => format!("{n} pictures couldn't be shown"),
            };
        }
    }

    /// The pictures `ops` place, which may need decoding: those an edit,
    /// undo or redo brings back.
    pub(super) fn pictures_in(ops: &[Op]) -> Vec<String> {
        let named = |runs: &[Run]| -> Vec<String> {
            runs.iter()
                .filter_map(|r| match r.props.get(keys::PICTURE) {
                    Some(Value::Text(id)) => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        let mut out = Vec::new();
        for op in ops {
            match op {
                Op::InsertText { runs, .. } => out.extend(named(runs)),
                Op::InsertBlock { objects, .. } => {
                    for (_, object) in objects {
                        if let doc::Object::Paragraph(p) = object {
                            out.extend(named(&p.runs));
                        }
                    }
                }
                Op::SetCharProp {
                    key,
                    value: Some(Value::Text(id)),
                    ..
                } if key == keys::PICTURE => out.push(id.clone()),
                _ => {}
            }
        }
        out
    }
}

/// A format as the document's file names it.
pub(super) fn format_name(format: Format) -> &'static str {
    match format {
        Format::Png => "png",
        Format::Jpeg => "jpeg",
        Format::Gif => "gif",
        Format::Webp => "webp",
    }
}

/// The format the document's file names `name`.
fn format_of(name: &str) -> Option<Format> {
    match name {
        "png" => Some(Format::Png),
        "jpeg" => Some(Format::Jpeg),
        "gif" => Some(Format::Gif),
        "webp" => Some(Format::Webp),
        _ => None,
    }
}
