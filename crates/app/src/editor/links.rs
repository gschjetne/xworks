//! Typing a web or e-mail address makes it a link (review jy4a2tx5), as
//! GitHub's autolinks read it (`markdown::autolinks`), once a space or
//! punctuation that ends it follows: an edit of its own, so Undo right
//! after takes the link off and leaves the text. In a Markdown document it
//! must, or the file would read back otherwise; in an `.xw` document,
//! `spec/editing.ron` says.
//!
//! A period doesn't end one, as addresses have periods inside: an address
//! at a sentence's end is linked by the space or line's end after it.
//! Typing on in an address linked so, as after a `:` that turned out to
//! begin a port, takes the link along, as part of the typing.

use doc::{Document, ObjectId, Op, Props, Value};
use story::keys;

use super::{Editor, Position};

/// The name of the edit that links an address typed, as Undo names it.
pub(super) const LINK: &str = "Link";

/// Whether `c`, typed after an address, ends it and makes it a link.
fn ends_address(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | ';' | ':' | '!' | '?' | ')' | '"' | '\'')
}

/// Whether a character with `props` is plain text an address can be in:
/// not code, Markdown kept as written or a picture.
fn plain(props: &Props) -> bool {
    !props.contains_key(keys::CODE)
        && !props.contains_key(keys::SOURCE)
        && !props.contains_key(keys::PICTURE)
}

/// Each character of paragraph `id` of `document` with its properties.
fn characters(document: &Document, id: ObjectId) -> Vec<(char, Props)> {
    let Some(p) = document.paragraph(id) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(p.text.len());
    let mut chars = p.text.chars();
    for run in &p.runs {
        for c in chars.by_ref().take(run.len as usize) {
            out.push((c, (*run.props).clone()));
        }
    }
    out
}

/// The run of characters that aren't whitespace around `at` in `chars`:
/// its start and end.
fn token(chars: &[(char, Props)], at: usize) -> (usize, usize) {
    let mut start = at.min(chars.len());
    while start > 0 && !chars[start - 1].0.is_whitespace() {
        start -= 1;
    }
    let mut end = at.min(chars.len());
    while end < chars.len() && !chars[end].0.is_whitespace() {
        end += 1;
    }
    (start, end)
}

/// The link on a character with `props`, if it has one.
fn link_of(props: &Props) -> Option<&str> {
    match props.get(keys::LINK) {
        Some(Value::Text(url)) => Some(url),
        _ => None,
    }
}

/// The links in `chars[start..end]` that are autolinks, whose text is the
/// address they link to, each as its range and address.
fn autolinked(chars: &[(char, Props)], start: usize, end: usize) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        let Some(url) = link_of(&chars[i].1) else {
            i += 1;
            continue;
        };
        let mut j = i;
        while j < end && link_of(&chars[j].1) == Some(url) {
            j += 1;
        }
        let text: String = chars[i..j].iter().map(|(c, _)| *c).collect();
        if markdown::address(&text).as_deref() == Some(url) {
            out.push((i, j, url.to_string()));
        }
        i = j;
    }
    out
}

impl Editor {
    /// Whether typing an address makes it a link in this document.
    pub(super) fn links_as_typed(&self) -> bool {
        let choice = &self.res.editing.links_as_typed;
        if self.is_markdown() {
            choice.markdown
        } else {
            choice.xworks
        }
    }

    /// What typing `n` characters at `at` in `before`, by `ops`, does to
    /// an autolink it types on in: the operations, to go with the typing,
    /// that make the link the address its text now reads as, or none.
    pub(super) fn relink(&self, before: &Document, ops: &[Op], at: Position, n: u32) -> Vec<Op> {
        if !self.links_as_typed() || self.is_code(at.paragraph) {
            return Vec::new();
        }
        let had = characters(before, at.paragraph);
        let (s0, e0) = token(&had, at.offset as usize);
        let auto = autolinked(&had, s0, e0);
        if auto.is_empty() {
            return Vec::new();
        }
        let mut typed = before.clone();
        if typed.apply_all(ops).is_err() {
            return Vec::new();
        }
        let chars = characters(&typed, at.paragraph);
        let caret = (at.offset + n) as usize;
        let (s, e) = token(&chars, caret.saturating_sub(1).max(at.offset as usize));
        if chars[s..e].iter().any(|(_, p)| !plain(p)) {
            return Vec::new();
        }
        // Where the links were, and what was typed, in the typed text.
        let shift = |i: usize| {
            if i >= at.offset as usize {
                i + n as usize
            } else {
                i
            }
        };
        let mut touched: Vec<(usize, usize)> = auto
            .iter()
            .map(|&(a, b, _)| (shift(a), shift(b - 1) + 1))
            .collect();
        touched.push((at.offset as usize, caret));
        let urls: Vec<&str> = auto.iter().map(|(_, _, u)| u.as_str()).collect();
        // Each character's link as it should be: the autolinks' taken off,
        // and those the token reads as, where they meet them, put on.
        let mut want: Vec<Option<String>> = chars[s..e]
            .iter()
            .map(|(_, p)| link_of(p).filter(|u| !urls.contains(u)).map(str::to_string))
            .collect();
        let text: String = chars[s..e].iter().map(|(c, _)| *c).collect();
        for link in markdown::autolinks(&text) {
            let (a, b) = (s + link.start, s + link.end);
            if touched.iter().any(|&(x, y)| x < b && a < y) {
                for w in &mut want[link.start..link.end] {
                    *w = Some(link.url.clone());
                }
            }
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < want.len() {
            let have = link_of(&chars[s + i].1).map(str::to_string);
            if have == want[i] {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < want.len()
                && want[j] == want[i]
                && link_of(&chars[s + j].1).map(str::to_string) != want[j]
            {
                j += 1;
            }
            out.push(Op::SetCharProp {
                paragraph: at.paragraph,
                offset: (s + i) as u32,
                len: (j - i) as u32,
                key: keys::LINK.to_string(),
                value: want[i].clone().map(Value::Text),
            });
            i = j;
        }
        out
    }

    /// After `typed` was typed, ending at the caret: if its last character
    /// ends an address, links it, as an edit of its own.
    pub(super) fn link_typed(&mut self, typed: &str) {
        let Some(last) = typed.chars().last().filter(|&c| ends_address(c)) else {
            return;
        };
        let caret = self.focus;
        let trigger = (caret.offset as usize).saturating_sub(1);
        // The address before whitespace, or before the punctuation and
        // with it, which tells where the address ends.
        let end = if last.is_whitespace() {
            trigger
        } else {
            caret.offset as usize
        };
        self.link_ending(caret.paragraph, end, trigger);
    }

    /// Links the addresses, as GitHub's autolinks read them, in the run
    /// of characters that aren't whitespace that ends at `end` in
    /// paragraph `p`, those that end by `limit` and aren't links yet: as
    /// an edit of its own, which Undo takes back.
    pub(super) fn link_ending(&mut self, p: ObjectId, end: usize, limit: usize) {
        if !self.links_as_typed() || self.document.paragraph(p).is_none() || self.is_code(p) {
            return;
        }
        let chars = characters(&self.document, p);
        let end = end.min(chars.len());
        let (start, _) = token(&chars, end);
        let text: String = chars[start..end].iter().map(|(c, _)| *c).collect();
        let mut ops = Vec::new();
        for link in markdown::autolinks(&text) {
            let (a, b) = (start + link.start, start + link.end);
            if b > limit
                || chars[a..b]
                    .iter()
                    .any(|(_, props)| !plain(props) || link_of(props).is_some())
            {
                continue;
            }
            ops.push(Op::SetCharProp {
                paragraph: p,
                offset: a as u32,
                len: (b - a) as u32,
                key: keys::LINK.to_string(),
                value: Some(Value::Text(link.url)),
            });
        }
        if ops.is_empty() {
            return;
        }
        let (anchor, focus) = (self.anchor, self.focus);
        if self.commit(LINK, ops, false) {
            // What's typed next is typing of its own, not this.
            self.typing = None;
            self.anchor = anchor;
            self.focus = focus;
        }
    }
}
