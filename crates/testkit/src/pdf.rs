//! A PDF's text read back, for tests: a second reader beside pdf-extract,
//! which misreads some Arabic (docs/ROADMAP.md, M2 stage 4).
//!
//! [`text`] walks each page's content in the order it's written, and
//! decodes the strings its text operators show through their fonts'
//! ToUnicode maps, as lopdf reads them. Where a marked-content sequence
//! carries `/ActualText`, as krilla writes for a cluster whose glyphs
//! don't map to its characters one for one, the actual text stands for
//! everything shown inside it.

use std::collections::BTreeMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Encoding, Object};

use crate::Error;

/// The text of every page of `pdf`, in the order each page writes it, a
/// page to a line.
pub fn text(pdf: &[u8]) -> Result<String, Error> {
    let document = Document::load_mem(pdf)?;
    let mut out = String::new();
    for (_, page) in document.get_pages() {
        let fonts = document.get_page_fonts(page)?;
        let encodings: BTreeMap<Vec<u8>, Encoding<'_>> = fonts
            .iter()
            .filter_map(|(name, font)| {
                font.get_font_encoding(&document)
                    .ok()
                    .map(|e| (name.clone(), e))
            })
            .collect();
        let content = Content::decode(&document.get_page_content(page)?)?;
        let mut reader = Reader {
            document: &document,
            encodings: &encodings,
            font: None,
            actual: Vec::new(),
            out: &mut out,
        };
        for operation in &content.operations {
            reader.operate(&operation.operator, &operation.operands);
        }
        out.push('\n');
    }
    Ok(out)
}

/// What reading a page's content keeps track of.
struct Reader<'a, 'd> {
    document: &'d Document,
    encodings: &'a BTreeMap<Vec<u8>, Encoding<'d>>,
    /// The font text is shown in, by its resource name.
    font: Option<Vec<u8>>,
    /// For each marked-content sequence open, whether it has actual text,
    /// which stands for what it shows.
    actual: Vec<bool>,
    out: &'a mut String,
}

impl Reader<'_, '_> {
    fn operate(&mut self, operator: &str, operands: &[Object]) {
        match operator {
            "Tf" => {
                self.font = operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .map(<[u8]>::to_vec);
            }
            "Tj" | "'" => {
                if let Some(s) = operands.last() {
                    self.show(s);
                }
            }
            "\"" => {
                if let Some(s) = operands.get(2) {
                    self.show(s);
                }
            }
            "TJ" => {
                if let Some(Object::Array(parts)) = operands.first() {
                    for part in parts {
                        self.show(part);
                    }
                }
            }
            "BMC" => self.actual.push(false),
            "BDC" => {
                let actual = operands
                    .get(1)
                    .and_then(|p| self.properties(p))
                    .and_then(|p| p.get(b"ActualText").ok().cloned());
                match actual {
                    Some(text) => {
                        if let Ok(text) = lopdf::decode_text_string(&text) {
                            self.out.push_str(&text);
                        }
                        self.actual.push(true);
                    }
                    None => self.actual.push(false),
                }
            }
            "EMC" => {
                self.actual.pop();
            }
            _ => {}
        }
    }

    /// The properties of a marked-content sequence, given inline or by
    /// name.
    fn properties(&self, object: &Object) -> Option<Dictionary> {
        match object {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(id) => self
                .document
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .cloned(),
            _ => None,
        }
    }

    /// Adds the text a string shows, unless actual text stands for it.
    fn show(&mut self, object: &Object) {
        if self.actual.iter().any(|&a| a) {
            return;
        }
        let Object::String(bytes, _) = object else {
            return;
        };
        let Some(encoding) = self.font.as_ref().and_then(|f| self.encodings.get(f)) else {
            return;
        };
        if let Ok(text) = Document::decode_text(encoding, bytes) {
            self.out.push_str(&text);
        }
    }
}
