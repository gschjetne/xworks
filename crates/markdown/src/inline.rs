//! A paragraph's text written as Markdown's inline content: its marks as
//! emphasis, strong, strikethrough, code spans and links, its pictures as
//! images, and its characters escaped where Markdown would read them as
//! more than text.

use std::collections::BTreeSet;

use doc::{Props, Value};
use story::{OBJECT, keys};

use crate::Reference;

/// How inline content is written.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Style<'a> {
    /// Emphasis: `*` or `_`.
    pub emphasis: char,
    /// Strong emphasis: `*` or `_`, doubled.
    pub strong: char,
    /// Whether every punctuation mark that could mean something is
    /// escaped, rather than those that would here.
    pub careful: bool,
    /// Whether it's a table's cell: its pipes escaped, even in code.
    pub cell: bool,
    /// Whether line breaks are two spaces at a line's end, rather than a
    /// backslash.
    pub spaced_breaks: bool,
    /// The links its paragraph wrote by reference, which it writes so
    /// again.
    pub references: &'a [Reference],
}

/// A mark that delimits text.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Mark {
    Strong,
    Emphasis,
    Strike,
    Link(String, String),
}

/// A stretch of a paragraph written as one thing: a character of text, a
/// code span, Markdown kept as it was, a picture, or a line break; with
/// the marks around it.
#[derive(Clone, Debug)]
struct Atom {
    /// Its first character.
    start: usize,
    what: AtomKind,
    marks: Vec<Mark>,
}

#[derive(Clone, Debug)]
enum AtomKind {
    Text(char),
    Code(String),
    Source(String),
    Picture {
        src: String,
        alt: String,
        title: String,
    },
    Break,
}

/// The marks on a character with `props`, outermost first as far as that
/// goes: links, then strong, emphasis and strikethrough.
fn marks_of(props: &Props) -> Vec<Mark> {
    let mut out = Vec::new();
    if let Some(Value::Text(url)) = props.get(keys::LINK) {
        let title = match props.get(keys::LINK_TITLE) {
            Some(Value::Text(t)) => t.clone(),
            _ => String::new(),
        };
        out.push(Mark::Link(url.clone(), title));
    }
    let on = |key| matches!(props.get(key), Some(Value::Bool(true)));
    if on(keys::BOLD) {
        out.push(Mark::Strong);
    }
    if on(keys::ITALIC) {
        out.push(Mark::Emphasis);
    }
    if on(keys::STRIKETHROUGH) {
        out.push(Mark::Strike);
    }
    out
}

fn text_of(props: &Props, key: &str) -> String {
    match props.get(key) {
        Some(Value::Text(t)) => t.clone(),
        _ => String::new(),
    }
}

/// `chars[from..to]` as atoms. `None` if one can't be written: a line
/// break in code.
fn atoms(chars: &[(char, Props)], from: usize, to: usize) -> Option<Vec<Atom>> {
    let mut out: Vec<Atom> = Vec::new();
    let mut i = from;
    while i < to {
        let (c, props) = &chars[i];
        let marks = marks_of(props);
        if *c == OBJECT && props.contains_key(keys::PICTURE_SRC) {
            out.push(Atom {
                start: i,
                what: AtomKind::Picture {
                    src: text_of(props, keys::PICTURE_SRC),
                    alt: text_of(props, keys::PICTURE_ALT),
                    title: text_of(props, keys::PICTURE_TITLE),
                },
                marks,
            });
            i += 1;
            continue;
        }
        let code = matches!(props.get(keys::CODE), Some(Value::Bool(true)));
        let source = props.get(keys::SOURCE).cloned();
        if code || source.is_some() {
            // As far as the same marks and the same kind of text go.
            let mut j = i;
            let mut text = String::new();
            while j < to {
                let (c, p) = &chars[j];
                let same = marks_of(p) == marks
                    && matches!(p.get(keys::CODE), Some(Value::Bool(true))) == code
                    && p.get(keys::SOURCE) == source.as_ref()
                    && !(*c == OBJECT && p.contains_key(keys::PICTURE_SRC));
                if !same {
                    break;
                }
                text.push(*c);
                j += 1;
            }
            let what = if source.is_some() {
                AtomKind::Source(text)
            } else {
                if text.contains('\u{2028}') {
                    return None;
                }
                AtomKind::Code(text)
            };
            out.push(Atom {
                start: i,
                what,
                marks,
            });
            i = j;
            continue;
        }
        let what = if *c == '\u{2028}' {
            AtomKind::Break
        } else {
            AtomKind::Text(*c)
        };
        out.push(Atom {
            start: i,
            what,
            marks,
        });
        i += 1;
    }
    Some(out)
}

/// A line written, and where in it the spaces are that it could break at:
/// each one's byte offset in the line and its character in the
/// paragraph.
#[derive(Clone, Debug, Default)]
pub(crate) struct Written {
    pub text: String,
    pub spaces: Vec<(usize, usize)>,
    /// Whether it starts with a character of text, which an escape may go
    /// before, rather than a mark.
    pub starts_with_text: bool,
}

/// Writes `chars[from..to]`, each a character and the properties Markdown
/// holds of it, as lines: broken at the spaces at `breaks` and after its
/// line breaks. `None` if it can't be written: a line break in code or in
/// a table's cell.
pub(crate) fn write(
    chars: &[(char, Props)],
    from: usize,
    to: usize,
    breaks: &BTreeSet<usize>,
    style: Style<'_>,
) -> Option<Vec<Written>> {
    let atoms = atoms(chars, from, to)?;
    if style.cell && atoms.iter().any(|a| matches!(a.what, AtomKind::Break)) {
        return None;
    }
    let mut w = Writer {
        chars,
        atoms: &atoms,
        style,
        lines: Vec::new(),
        line: Written::default(),
        open: Vec::new(),
        links: Vec::new(),
        chosen: Vec::new(),
        link_depth: 0,
        bracket: chars.iter().rposition(|x| x.0 == ']'),
    };
    let mut k = 0;
    while k < atoms.len() {
        let atom = &atoms[k];
        w.marks_at(k);
        // A link written as an autolink takes its whole span.
        if let Some(n) = w.autolink(k) {
            k = n;
            continue;
        }
        match &atom.what {
            AtomKind::Text(c) => {
                if breaks.contains(&atom.start) && *c == ' ' {
                    w.new_line();
                } else {
                    w.text(atom.start, *c);
                }
            }
            AtomKind::Break => {
                w.raw(if style.spaced_breaks { "  " } else { "\\" });
                w.new_line();
            }
            AtomKind::Code(text) => w.code(text),
            AtomKind::Source(text) => {
                // Kept as written, but for its line breaks, which are the
                // lines it was written on.
                for (i, part) in text.split('\u{2028}').enumerate() {
                    if i > 0 {
                        w.new_line();
                    }
                    w.raw(part);
                }
            }
            AtomKind::Picture { src, alt, title } => {
                w.guard_bang();
                w.raw("![");
                w.link_depth += 1;
                let alt: Vec<(char, Props)> = alt.chars().map(|c| (c, Props::new())).collect();
                for i in 0..alt.len() {
                    let escaped = escape(&alt, i, true, style, None);
                    w.raw(&escaped);
                }
                w.link_depth -= 1;
                w.raw("](");
                w.raw(&destination(src));
                if !title.is_empty() {
                    w.raw(" ");
                    w.raw(&quoted(title));
                }
                w.raw(")");
            }
        }
        k += 1;
    }
    w.close_to(0);
    w.lines.push(std::mem::take(&mut w.line));
    Some(w.lines)
}

struct Writer<'a> {
    chars: &'a [(char, Props)],
    atoms: &'a [Atom],
    style: Style<'a>,
    /// The atoms each open link spans, innermost last, and how many lines
    /// were written and how much of the line where its text starts.
    links: Vec<(usize, usize, usize, usize)>,
    lines: Vec<Written>,
    line: Written,
    /// The marks open, outermost first, with the delimiter each was opened
    /// with.
    open: Vec<Mark>,
    chosen: Vec<char>,
    /// How many links and pictures' descriptions are open, in which `]`
    /// ends something.
    link_depth: usize,
    /// Where the paragraph's last `]` is, which a `[` before might pair
    /// with.
    bracket: Option<usize>,
}

impl Writer<'_> {
    fn raw(&mut self, s: &str) {
        if self.line.text.is_empty() && !s.is_empty() {
            self.line.starts_with_text = false;
        }
        self.line.text.push_str(s);
    }

    fn new_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.line));
    }

    fn text(&mut self, at: usize, c: char) {
        let first = self.line.text.is_empty();
        if c == ' ' {
            self.line.spaces.push((self.line.text.len(), at));
        }
        let in_link = self.link_depth > 0;
        let escaped = escape(self.chars, at, in_link, self.style, self.bracket);
        self.line.text.push_str(&escaped);
        if first {
            self.line.starts_with_text = true;
        }
    }

    /// The span of mark `mark` from atom `k`: the atom after its last.
    fn span_end(&self, k: usize, mark: &Mark) -> usize {
        let mut j = k;
        while j < self.atoms.len() && self.atoms[j].marks.contains(mark) {
            j += 1;
        }
        j
    }

    /// Closes the marks open from `depth` in, innermost first.
    fn close_to(&mut self, depth: usize) {
        while self.open.len() > depth {
            let mark = self.open.pop().expect("open");
            let c = self.chosen.pop().unwrap_or('*');
            match mark {
                Mark::Strong => self.raw(&format!("{c}{c}")),
                Mark::Emphasis => self.raw(&c.to_string()),
                Mark::Strike => self.raw("~~"),
                Mark::Link(url, title) => {
                    self.link_depth -= 1;
                    let (from, to, line, at) = self.links.pop().unwrap_or((0, 0, 0, 0));
                    // By reference, as its paragraph had it, if it still
                    // goes where the reference does.
                    let reference = self
                        .style
                        .references
                        .iter()
                        .find(|r| r.url == url && r.title == title);
                    if let Some(r) = reference {
                        // A label is matched as it's written, escapes
                        // and all.
                        let written = (self.lines.len() == line)
                            .then(|| self.line.text.get(at..))
                            .flatten();
                        let shortcut = r.shortcut
                            && self.plain_text((from, to)).is_some_and(|t| {
                                label(&t) == label(&r.label) && written == Some(t.as_str())
                            });
                        if shortcut {
                            self.raw("]");
                        } else {
                            self.raw(&format!("][{}]", r.label));
                        }
                        continue;
                    }
                    self.raw("](");
                    self.raw(&destination(&url));
                    if !title.is_empty() {
                        self.raw(" ");
                        self.raw(&quoted(&title));
                    }
                    self.raw(")");
                }
            }
        }
    }

    /// Closes and opens marks so those open are atom `k`'s.
    fn marks_at(&mut self, k: usize) {
        let want = self.atoms[k].marks.clone();
        // Close from the outermost mark that ends here; those inside it
        // that go on open again.
        let keep = self
            .open
            .iter()
            .position(|m| !want.contains(m))
            .unwrap_or(self.open.len());
        self.close_to(keep);
        let mut opening: Vec<(usize, Mark)> = want
            .iter()
            .filter(|m| !self.open.contains(m))
            .map(|m| (self.span_end(k, m), m.clone()))
            .collect();
        // What goes on longest opens first, outside the rest; links
        // outside other marks that end with them.
        opening.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| matches!(b.1, Mark::Link(..)).cmp(&matches!(a.1, Mark::Link(..))))
        });
        for (end, mark) in opening {
            match &mark {
                Mark::Link(..) => {
                    self.guard_bang();
                    self.raw("[");
                    self.link_depth += 1;
                    self.links
                        .push((k, end, self.lines.len(), self.line.text.len()));
                    self.chosen.push('[');
                }
                Mark::Strong | Mark::Emphasis => {
                    let habit = if mark == Mark::Strong {
                        self.style.strong
                    } else {
                        self.style.emphasis
                    };
                    let c = self.delimiter(habit, k, end);
                    if mark == Mark::Strong {
                        self.raw(&format!("{c}{c}"));
                    } else {
                        self.raw(&c.to_string());
                    }
                    self.chosen.push(c);
                }
                Mark::Strike => {
                    self.raw("~~");
                    self.chosen.push('~');
                }
            }
            self.open.push(mark);
        }
    }

    /// The delimiter emphasis from atom `k` to before atom `end` is
    /// written with: the habit's, unless an underscore can't open or
    /// close there, inside a word, or the same character is just before.
    fn delimiter(&self, habit: char, k: usize, end: usize) -> char {
        let before = self.atoms[k].start.checked_sub(1).map(|i| self.chars[i].0);
        let after = self
            .atoms
            .get(end)
            .map(|a| a.start)
            .and_then(|i| self.chars.get(i))
            .map(|c| c.0);
        let wordy = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
        let underscore_fits = !wordy(before) && !wordy(after);
        let last = self.line.text.chars().last();
        let mut c = habit;
        if c == '_' && !underscore_fits {
            c = '*';
        }
        if last == Some(c) {
            let other = if c == '*' { '_' } else { '*' };
            if other == '*' || underscore_fits {
                c = other;
            }
        }
        c
    }

    /// An exclamation mark just written before a `[` that opens a link
    /// would make it a picture: it's escaped.
    fn guard_bang(&mut self) {
        if self.line.text.ends_with('!') && !self.line.text.ends_with("\\!") {
            self.line.text.pop();
            self.line.text.push_str("\\!");
        }
    }

    /// The text of atoms `span`, if it's text alone with no marks but its
    /// link's.
    fn plain_text(&self, (from, to): (usize, usize)) -> Option<String> {
        let mut out = String::new();
        for atom in self.atoms.get(from..to)? {
            match atom.what {
                AtomKind::Text(c) if atom.marks.len() == 1 => out.push(c),
                _ => return None,
            }
        }
        Some(out)
    }

    /// A code span holding `text`, in enough backticks.
    fn code(&mut self, text: &str) {
        let mut longest = 0;
        let mut run = 0;
        for c in text.chars() {
            if c == '`' {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
        let fence = "`".repeat(longest + 1);
        let all_spaces = text.chars().all(|c| c == ' ');
        let pad = text.starts_with('`')
            || text.ends_with('`')
            || (!all_spaces && text.starts_with(' ') && text.ends_with(' '));
        let body = if self.style.cell {
            text.replace('|', "\\|")
        } else {
            text.to_string()
        };
        let space = if pad { " " } else { "" };
        self.raw(&format!("{fence}{space}{body}{space}{fence}"));
    }

    /// If atom `k` opens a link whose text is its target, as plain text
    /// all of one kind, writes it as an autolink and returns the atom
    /// after it.
    fn autolink(&mut self, k: usize) -> Option<usize> {
        let Some(Mark::Link(url, title)) = self.open.last().cloned() else {
            return None;
        };
        // Only just opened: nothing written inside it yet.
        if !self.line.text.ends_with('[') || !title.is_empty() {
            return None;
        }
        let end = self.span_end(k, &Mark::Link(url.clone(), title));
        let marks = &self.atoms[k].marks;
        let mut text = String::new();
        for atom in &self.atoms[k..end] {
            match atom.what {
                AtomKind::Text(c) if atom.marks == *marks => text.push(c),
                _ => return None,
            }
        }
        let fits = (url == text && is_uri(&url))
            || (url.strip_prefix("mailto:") == Some(text.as_str()) && is_email(&text));
        if !fits {
            return None;
        }
        let shown = text;
        self.line.text.pop();
        self.open.pop();
        self.chosen.pop();
        self.links.pop();
        self.link_depth -= 1;
        self.raw(&format!("<{shown}>"));
        Some(end)
    }
}

/// A link label as CommonMark matches them: case folded, its runs of
/// spaces one.
fn label(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `s` is a URI an autolink can be: a scheme of 2 to 32 letters,
/// digits, `+`, `.` and `-`, starting with a letter, a colon, and no
/// spaces, controls or angle brackets.
fn is_uri(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else {
        return false;
    };
    let mut cs = scheme.chars();
    (2..=32).contains(&scheme.len())
        && cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        && cs.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        && !rest
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
}

/// Whether `s` is an e-mail address an autolink can be.
fn is_email(s: &str) -> bool {
    let Some((user, host)) = s.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
        && !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// A link's or picture's destination, written: in angle brackets where it
/// has spaces, unmatched parentheses or nothing, and otherwise as it is;
/// backslashes and what would end it escaped.
pub(crate) fn destination(url: &str) -> String {
    let mut depth: i64 = 0;
    let mut balanced = true;
    for c in url.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    balanced = false;
                }
            }
            _ => {}
        }
    }
    balanced &= depth == 0;
    let angled = url.is_empty()
        || !balanced
        || url.starts_with('<')
        || url.chars().any(|c| c == ' ' || c.is_control());
    let mut out = String::new();
    if angled {
        out.push('<');
    }
    let chars: Vec<char> = url.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let escape = match c {
            '\\' => chars.get(i + 1).is_none_or(|n| n.is_ascii_punctuation()),
            '<' | '>' => angled,
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    if angled {
        out.push('>');
    }
    out
}

/// A title, in double quotes, with what would end it escaped.
pub(crate) fn quoted(title: &str) -> String {
    let mut out = String::from('"');
    let chars: Vec<char> = title.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' || (c == '\\' && chars.get(i + 1).is_none_or(|n| n.is_ascii_punctuation())) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Character `i` of `chars` as text, escaped where Markdown would read it
/// as more: in a link's text or a picture's description with `in_link`;
/// `bracket` is where the text's last `]` is.
fn escape(
    chars: &[(char, Props)],
    i: usize,
    in_link: bool,
    style: Style<'_>,
    bracket: Option<usize>,
) -> String {
    let c = chars[i].0;
    if c.is_control() && c != '\t' {
        // A line ending or another control character in text, as an
        // entity reads it back.
        return format!("&#{};", u32::from(c));
    }
    let prev = i.checked_sub(1).map(|j| chars[j].0);
    let next = chars.get(i + 1).map(|x| x.0);
    let blank = |c: Option<char>| c.is_none_or(|c| c.is_whitespace());
    let wordy = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let needs = if style.careful {
        "\\`*_[]<>!&#~|".contains(c)
    } else {
        match c {
            '\\' => next.is_none_or(|n| n.is_ascii_punctuation() || n == '\u{2028}' || n == ' '),
            '`' => true,
            '*' => !(blank(prev) && blank(next)),
            '_' => !((wordy(prev) && wordy(next)) || (blank(prev) && blank(next))),
            '~' => !(blank(prev) && blank(next)),
            '[' => in_link || bracket.is_some_and(|j| j > i),
            ']' => in_link,
            '<' => next.is_some_and(|n| n.is_ascii_alphabetic() || "/!?".contains(n)),
            '&' => entity_like(chars, i),
            '|' => style.cell,
            _ => false,
        }
    };
    let needs = needs || (c == '|' && style.cell);
    let mut out = String::new();
    if needs {
        out.push('\\');
    }
    out.push(c);
    out
}

/// Whether `&` at `i` starts what reads as an entity: `&name;` or
/// `&#123;`.
fn entity_like(chars: &[(char, Props)], i: usize) -> bool {
    let rest: String = chars[i + 1..].iter().take(34).map(|x| x.0).collect();
    let body: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '#')
        .collect();
    !body.is_empty() && rest[body.len()..].starts_with(';')
}

/// `line`, escaped where its start would begin a block: a heading, quote,
/// list item, thematic break, setext heading's underline, fence or table's
/// delimiter row. Only text at its start is escaped; marks there can't
/// begin a block.
pub(crate) fn guard_start(line: &Written) -> String {
    let text = &line.text;
    if !line.starts_with_text || text.is_empty() {
        return text.clone();
    }
    let first = text.chars().next().expect("not empty");
    let rest = &text[first.len_utf8()..];
    let gap = |s: &str| s.is_empty() || s.starts_with([' ', '\t']);
    let only = |s: &str, c: char| s.chars().all(|x| x == c || x == ' ' || x == '\t');
    // A table's delimiter row, under a line of cells, would make them a
    // table.
    let delimiter_row = text.contains('-') && text.chars().all(|c| "|:- \t".contains(c));
    let escape_first = delimiter_row
        || match first {
            '#' => {
                let hashes = rest.chars().take_while(|&c| c == '#').count();
                hashes < 6 && gap(&rest[hashes..])
            }
            '>' => true,
            '-' | '+' | '*' => {
                gap(rest)
                    || (first != '+' && thematic(text, first))
                    || (first == '-' && only(rest, '-'))
            }
            '_' => thematic(text, '_'),
            '=' => only(rest, '='),
            _ => false,
        };
    if escape_first {
        return format!("\\{text}");
    }
    // A number and a delimiter make an ordered list's item.
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let after = &text[digits..];
        if after.starts_with(['.', ')']) && gap(&after[1..]) {
            return format!("{}\\{}", &text[..digits], after);
        }
    }
    text.clone()
}

/// Whether `line` is a thematic break of `c`: three or more, and spaces.
fn thematic(line: &str, c: char) -> bool {
    line.chars().all(|x| x == c || x == ' ' || x == '\t')
        && line.chars().filter(|&x| x == c).count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Vec<(char, Props)> {
        text.chars().map(|c| (c, Props::new())).collect()
    }

    fn style() -> Style<'static> {
        Style {
            emphasis: '*',
            strong: '*',
            careful: false,
            cell: false,
            spaced_breaks: false,
            references: &[],
        }
    }

    fn one_line(chars: &[(char, Props)]) -> String {
        let lines = write(chars, 0, chars.len(), &BTreeSet::new(), style()).expect("writes");
        lines.iter().map(guard_start).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn text_is_escaped_only_where_it_would_mean_more() {
        assert_eq!(
            one_line(&plain("a * b and snake_case")),
            "a * b and snake_case"
        );
        assert_eq!(
            one_line(&plain("*not* [x] [1](y) 2 < 3 <b>")),
            "\\*not\\* \\[x] \\[1](y) 2 < 3 \\<b>"
        );
        assert_eq!(one_line(&plain("AT&T &amp; `x`")), "AT&T \\&amp; \\`x\\`");
        assert_eq!(one_line(&plain("# not a heading")), "\\# not a heading");
        assert_eq!(one_line(&plain("1. not a list")), "1\\. not a list");
        assert_eq!(one_line(&plain("- not an item")), "\\- not an item");
    }

    #[test]
    fn marks_nest_and_links_take_their_targets() {
        let bold = Props::from([(keys::BOLD.to_string(), Value::Bool(true))]);
        let mut italic_bold = bold.clone();
        italic_bold.insert(keys::ITALIC.to_string(), Value::Bool(true));
        let link = Props::from([(
            keys::LINK.to_string(),
            Value::Text("https://example.com".into()),
        )]);
        let mut chars = plain("a ");
        chars.extend("bold".chars().map(|c| (c, bold.clone())));
        chars.extend("both".chars().map(|c| (c, italic_bold.clone())));
        chars.extend(plain(" "));
        chars.extend("https://example.com".chars().map(|c| (c, link.clone())));
        chars.extend(plain(" and "));
        chars.extend("this".chars().map(|c| (c, link.clone())));
        assert_eq!(
            one_line(&chars),
            "a **bold*both*** <https://example.com> and [this](https://example.com)"
        );
    }

    #[test]
    fn code_takes_enough_backticks() {
        let code = Props::from([(keys::CODE.to_string(), Value::Bool(true))]);
        let chars: Vec<(char, Props)> = "a`b".chars().map(|c| (c, code.clone())).collect();
        assert_eq!(one_line(&chars), "``a`b``");
        let chars: Vec<(char, Props)> = "`x".chars().map(|c| (c, code.clone())).collect();
        assert_eq!(one_line(&chars), "`` `x ``");
    }

    #[test]
    fn destinations_and_titles_are_written_to_read_back() {
        assert_eq!(destination("a b.png"), "<a b.png>");
        assert_eq!(destination("x(1).png"), "x(1).png");
        assert_eq!(destination("x).png"), "<x).png>");
        assert_eq!(destination(""), "<>");
        assert_eq!(quoted("say \"hi\""), "\"say \\\"hi\\\"\"");
    }
}
