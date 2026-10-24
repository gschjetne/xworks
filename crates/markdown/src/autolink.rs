//! GitHub's autolinks (its "Autolinks (extension)"), which the owner took
//! after trying stage 6 (review jy4a2tx5): bare web addresses that start
//! `www.`, `http://` or `https://`, and e-mail addresses, found in plain
//! text, as pulldown-cmark doesn't find them.
//!
//! The rules, GitHub's:
//!
//! - An autolink starts a line, or comes after whitespace or one of `*`,
//!   `_`, `~` and `(`.
//! - `www.` and a valid domain links to the address with `http://` before
//!   it; `http://` or `https://` and a valid domain, to itself. A valid
//!   domain is segments of letters, digits, `_` and `-`, separated by
//!   periods, at least one, with no `_` in its last two segments. After
//!   it, anything but whitespace and `<` is the link's path.
//! - An e-mail address is letters, digits, `.`, `-`, `_` and `+`, an `@`,
//!   and segments of letters, digits, `-` and `_` separated by periods, at
//!   least one, not ending in `-` or `_`; it links to itself with
//!   `mailto:` before it.
//! - At a link's end, `?`, `!`, `.`, `,`, `:`, `*`, `_` and `~` aren't
//!   its, nor a `)` without a `(` in it to close, nor a `;` (with what
//!   looks like an entity reference before it, `&` and letters).
//!
//! A character that came from an escape or an entity isn't as written,
//! and what would be an autolink with one in it isn't one: so text that
//! would read as one is written with a character of it escaped.

/// What an autolink is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutolinkKind {
    /// A web address starting `www.`.
    Www,
    /// A web address starting `http://` or `https://`.
    Url,
    /// An e-mail address.
    Email,
}

/// An autolink found in text: where it is, in characters, what it is,
/// and the address it links to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Autolink {
    /// Its first character.
    pub start: usize,
    /// The character after its last.
    pub end: usize,
    /// What it is.
    pub kind: AutolinkKind,
    /// The address it links to.
    pub url: String,
}

impl Autolink {
    /// The character of it that, escaped, makes its text no autolink:
    /// a web address's `.` after `www` or `:` after its scheme, an
    /// e-mail address's `@`.
    pub(crate) fn breaker(&self, text: &[char]) -> usize {
        let find = |c: char| {
            (self.start..self.end)
                .find(|&i| text[i] == c)
                .unwrap_or(self.start)
        };
        match self.kind {
            AutolinkKind::Www => self.start + 3,
            AutolinkKind::Url => find(':'),
            AutolinkKind::Email => find('@'),
        }
    }
}

/// The autolinks in `text`, a line of plain text, each character of it
/// as written: as GitHub finds them.
pub fn autolinks(text: &str) -> Vec<Autolink> {
    let chars: Vec<char> = text.chars().collect();
    let clean = vec![true; chars.len()];
    find(&chars, &clean, None)
}

/// The address `text` links to if it's an autolink and nothing else.
pub fn address(text: &str) -> Option<String> {
    let found = autolinks(text);
    match found.as_slice() {
        [link] if link.start == 0 && link.end == text.chars().count() => Some(link.url.clone()),
        _ => None,
    }
}

/// The autolinks in `text`, plain text with no line breaks, whose
/// characters `clean` says are each as written; `before` is the character
/// before it, none at a line's start.
pub(crate) fn find(text: &[char], clean: &[bool], before: Option<char>) -> Vec<Autolink> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let prev = if i == 0 { before } else { Some(text[i - 1]) };
        if starts(prev) {
            let limit = text.len();
            let found = www(text, i, limit)
                .or_else(|| url(text, i, limit))
                .or_else(|| email(text, i, limit));
            // One with a character that isn't as written isn't one; cut
            // short there, what's left might be.
            if let Some(link) = found.filter(|l| clean[l.start..l.end].iter().all(|&c| c)) {
                i = link.end;
                out.push(link);
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Whether an autolink may start after `prev`: at a line's start, after
/// whitespace, or after `*`, `_`, `~` or `(`.
fn starts(prev: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '('))
}

/// Whether `c` can be in a web address's domain.
fn domain_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// Whether `domain`, its periods at the end left out, is a valid domain:
/// segments separated by periods, at least one, none empty, and no `_` in
/// the last two.
fn valid_domain(domain: &[char]) -> bool {
    let end = domain.len() - domain.iter().rev().take_while(|&&c| c == '.').count();
    let segments: Vec<&[char]> = domain[..end].split(|&c| c == '.').collect();
    segments.len() >= 2
        && segments.iter().all(|s| !s.is_empty())
        && segments.iter().rev().take(2).all(|s| !s.contains(&'_'))
}

/// A web address from `domain`, where its domain starts, to before
/// `limit`: the domain, the path after it, and the punctuation at its end
/// left off; none if the domain isn't valid.
fn web(text: &[char], start: usize, domain: usize, limit: usize) -> Option<usize> {
    let mut end = domain;
    while end < limit && domain_char(text[end]) {
        end += 1;
    }
    if !valid_domain(&text[domain..end]) {
        return None;
    }
    while end < limit && !text[end].is_whitespace() && text[end] != '<' {
        end += 1;
    }
    let end = trim(text, start, end);
    (end > domain).then_some(end)
}

/// A web address starting `www.` at `start`.
fn www(text: &[char], start: usize, limit: usize) -> Option<Autolink> {
    if !text[start..limit].starts_with(&['w', 'w', 'w', '.']) {
        return None;
    }
    let end = web(text, start, start, limit)?;
    if end <= start + 4 {
        return None;
    }
    let shown: String = text[start..end].iter().collect();
    Some(Autolink {
        start,
        end,
        kind: AutolinkKind::Www,
        url: format!("http://{shown}"),
    })
}

/// A web address starting `http://` or `https://` at `start`.
fn url(text: &[char], start: usize, limit: usize) -> Option<Autolink> {
    let rest = &text[start..limit];
    let scheme = ["http://", "https://"].into_iter().find(|s| {
        rest.len() >= s.len()
            && rest
                .iter()
                .zip(s.chars())
                .all(|(a, b)| a.eq_ignore_ascii_case(&b))
    })?;
    let domain = start + scheme.len();
    let end = web(text, start, domain, limit)?;
    Some(Autolink {
        start,
        end,
        kind: AutolinkKind::Url,
        url: text[start..end].iter().collect(),
    })
}

/// An e-mail address at `start`.
fn email(text: &[char], start: usize, limit: usize) -> Option<Autolink> {
    let local = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+');
    let at = (start..limit).find(|&i| !local(text[i]))?;
    if at == start || text[at] != '@' {
        return None;
    }
    let mut end = at + 1;
    while end < limit && (text[end].is_ascii_alphanumeric() || matches!(text[end], '-' | '_' | '.'))
    {
        end += 1;
    }
    // Periods at its end aren't its.
    while end > at + 1 && text[end - 1] == '.' {
        end -= 1;
    }
    let domain = &text[at + 1..end];
    let segments: Vec<&[char]> = domain.split(|&c| c == '.').collect();
    let valid = segments.len() >= 2
        && segments.iter().all(|s| !s.is_empty())
        && !domain.last().is_some_and(|&c| c == '-' || c == '_');
    if !valid {
        return None;
    }
    let shown: String = text[start..end].iter().collect();
    Some(Autolink {
        start,
        end,
        kind: AutolinkKind::Email,
        url: format!("mailto:{shown}"),
    })
}

/// Where a web address from `start` to `end` ends, without the
/// punctuation at its end that isn't its.
fn trim(text: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        match text[end - 1] {
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' => end -= 1,
            ')' => {
                let link = &text[start..end];
                let open = link.iter().filter(|&&c| c == '(').count();
                let close = link.iter().filter(|&&c| c == ')').count();
                if close <= open {
                    return end;
                }
                end -= 1;
            }
            ';' => {
                // What looks like an entity reference, `&` and letters and
                // digits, isn't its either.
                let mut k = end - 1;
                while k > start && text[k - 1].is_ascii_alphanumeric() {
                    k -= 1;
                }
                if k < end - 1 && k > start && text[k - 1] == '&' {
                    end = k - 1;
                } else {
                    end -= 1;
                }
            }
            _ => return end,
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, String)> {
        let chars: Vec<char> = text.chars().collect();
        autolinks(text)
            .into_iter()
            .map(|l| (chars[l.start..l.end].iter().collect(), l.url))
            .collect()
    }

    #[test]
    fn web_addresses_and_their_ends() {
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            found("See www.example.org/garden, then"),
            [pair(
                "www.example.org/garden",
                "http://www.example.org/garden"
            )]
        );
        assert_eq!(
            found("www.example"),
            [pair("www.example", "http://www.example")]
        );
        assert_eq!(found("www."), []);
        assert_eq!(found("awww.example.org"), []);
        assert_eq!(found("www.a.b_c"), []);
        assert_eq!(
            found("(https://example.org/a_(b))"),
            [pair(
                "https://example.org/a_(b)",
                "https://example.org/a_(b)"
            )]
        );
        assert_eq!(found("http://localhost"), []);
        assert_eq!(
            found("www.example.org/a?b=1&lt;"),
            [pair(
                "www.example.org/a?b=1",
                "http://www.example.org/a?b=1"
            )]
        );
    }

    #[test]
    fn email_addresses() {
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            found("Mail me@example.org."),
            [pair("me@example.org", "mailto:me@example.org")]
        );
        assert_eq!(found("me@example-"), []);
        assert_eq!(found("me+x@a.b_"), []);
        assert_eq!(found("x!me@a.b"), []);
        assert_eq!(address("me@a.b"), Some("mailto:me@a.b".to_string()));
        assert_eq!(address("me@a.b."), None);
    }
}
