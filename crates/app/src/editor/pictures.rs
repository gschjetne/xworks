//! Pictures placed in the text: Insert Picture asks the shell for a file
//! ([`Effect::Choose`]), and the file chosen is read, decoded, kept in the
//! document's file once by its hash, and placed at the caret as a
//! character whose run names it (`story::keys::PICTURE`). The pictures a
//! document shows are decoded when it opens, and when an edit, undo or
//! redo brings back one not decoded yet.

use std::path::Path;

use doc::{Op, Props, Run, Value};
use rayon::prelude::*;
use story::{OBJECT, keys};
use text::units::PIXEL;

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
        if let Some(why) = self.store.read_only() {
            self.message = format!("Read-only: {why}");
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
        let id = match self.store.add_image(format_name(decoded.format), &bytes) {
            Ok(id) => id,
            Err(e) => {
                self.message = format!("Couldn't place {name}: {e}");
                return;
            }
        };
        self.pictures.insert(id.clone(), decoded.image);
        let (width, height) = self.placed_size(decoded.width, decoded.height);
        let alt = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let mut props = Props::new();
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
        }
    }

    /// The size a picture `width` by `height` pixels is placed at: at 96
    /// pixels to the inch, scaled down to fit the page's body.
    fn placed_size(&self, width: u32, height: u32) -> (i64, i64) {
        let (w, h) = (i64::from(width) * PIXEL, i64::from(height) * PIXEL);
        let page = &self.res.page;
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
        let stored: Vec<(String, Option<store::Image>)> = wanted
            .into_iter()
            .map(|id| {
                let image = self.store.image(&id).ok().flatten();
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
fn format_name(format: Format) -> &'static str {
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
