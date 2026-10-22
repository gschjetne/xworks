//! Hyphenation by Liang's algorithm, from TeX's pattern files (hyph-utf8),
//! read as they ship.
//!
//! A pattern such as `.ach4` or `a1b` is letters with digits between them.
//! To hyphenate a word, every pattern found in it, the word's ends marked
//! by `.`, puts its digits on the gaps it covers; each gap keeps the
//! highest, and the word may break at gaps whose number is odd (Frank
//! Liang, *Word Hy-phen-a-tion by Com-put-er*, 1983). Exceptions list
//! words with their hyphens written in, and words that never break.

use std::collections::BTreeMap;
use std::sync::Arc;

use locale::Language;

/// One language's hyphenation: its patterns, its exceptions, and how many
/// letters must stay on each side of a break.
#[derive(Clone, Default)]
pub struct Hyphenator {
    /// The patterns' letters as a trie; node 0 is the root.
    nodes: Vec<Node>,
    /// Exceptions, lower case without hyphens, and where they break, in
    /// characters from their start.
    exceptions: BTreeMap<String, Vec<usize>>,
    left: usize,
    right: usize,
}

#[derive(Clone, Default)]
struct Node {
    /// Its children by letter, sorted.
    children: Vec<(char, u32)>,
    /// The digits of the pattern ending here, one more than its letters;
    /// empty if none does.
    values: Vec<u8>,
}

impl std::fmt::Debug for Hyphenator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hyphenator")
            .field("nodes", &self.nodes.len())
            .field("exceptions", &self.exceptions.len())
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

impl Hyphenator {
    /// A hyphenator from `patterns`, as a TeX pattern file (`.pat.txt`)
    /// holds them, and `exceptions`, as an exceptions file (`.hyp.txt`)
    /// does: words separated by white space. A break leaves at least
    /// `left` letters before it and `right` after it.
    ///
    /// It reads any text: what isn't a pattern, such as a `%` comment, is
    /// skipped.
    pub fn parse(patterns: &str, exceptions: &str, left: usize, right: usize) -> Hyphenator {
        let mut hyphenator = Hyphenator {
            nodes: vec![Node::default()],
            exceptions: BTreeMap::new(),
            left: left.max(1),
            right: right.max(1),
        };
        for word in words(patterns) {
            let mut letters = Vec::new();
            let mut values = vec![0u8];
            for c in word.chars() {
                match c.to_digit(10) {
                    Some(d) => {
                        if let Some(last) = values.last_mut() {
                            *last = d as u8;
                        }
                    }
                    None => {
                        letters.push(c);
                        values.push(0);
                    }
                }
            }
            if !letters.is_empty() {
                hyphenator.insert(&letters, values);
            }
        }
        for word in words(exceptions) {
            let mut key = String::new();
            let mut breaks = Vec::new();
            let mut count = 0;
            for c in word.chars() {
                if c == '-' {
                    if count > 0 && breaks.last() != Some(&count) {
                        breaks.push(count);
                    }
                } else {
                    key.push(lower(c));
                    count += 1;
                }
            }
            breaks.retain(|&b| b < count);
            if !key.is_empty() {
                hyphenator.exceptions.insert(key, breaks);
            }
        }
        hyphenator
    }

    fn insert(&mut self, letters: &[char], values: Vec<u8>) {
        let mut node = 0;
        for &c in letters {
            let children = &self.nodes[node].children;
            node = match children.binary_search_by_key(&c, |&(k, _)| k) {
                Ok(i) => children[i].1 as usize,
                Err(i) => {
                    let new = self.nodes.len();
                    self.nodes.push(Node::default());
                    self.nodes[node].children.insert(i, (c, new as u32));
                    new
                }
            };
        }
        self.nodes[node].values = values;
    }

    fn child(&self, node: usize, c: char) -> Option<usize> {
        let children = &self.nodes[node].children;
        children
            .binary_search_by_key(&c, |&(k, _)| k)
            .ok()
            .map(|i| children[i].1 as usize)
    }

    /// Where `word` may break, as byte offsets into it, in order. `word`
    /// is letters alone, in any case; a word the exceptions list breaks as
    /// they say.
    pub fn hyphenate(&self, word: &str) -> Vec<usize> {
        let offsets: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
        let n = offsets.len();
        if n < self.left + self.right {
            return Vec::new();
        }
        let letters: Vec<char> = word.chars().map(lower).collect();
        let at = |breaks: &mut dyn Iterator<Item = usize>| -> Vec<usize> {
            breaks
                .filter(|&k| k >= self.left && k + self.right <= n)
                .map(|k| offsets[k])
                .collect()
        };
        let key: String = letters.iter().collect();
        if let Some(breaks) = self.exceptions.get(&key) {
            return at(&mut breaks.iter().copied());
        }
        // The word between dots, and the highest digit on each gap: gap k
        // is before the kth of them.
        let mut marked = Vec::with_capacity(n + 2);
        marked.push('.');
        marked.extend(&letters);
        marked.push('.');
        let mut points = vec![0u8; marked.len() + 1];
        for start in 0..marked.len() {
            let mut node = 0;
            for &c in &marked[start..] {
                let Some(next) = self.child(node, c) else {
                    break;
                };
                node = next;
                for (k, &v) in self.nodes[node].values.iter().enumerate() {
                    let p = &mut points[start + k];
                    *p = (*p).max(v);
                }
            }
        }
        // The break before letter k is gap k + 1, after the leading dot.
        at(&mut (1..n).filter(|&k| points[k + 1] % 2 == 1))
    }
}

/// `c` in lower case, where that's one character; otherwise `c`.
fn lower(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// The words of `text`, separated by white space, without `%` comments.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|line| line.split('%').next().unwrap_or(""))
        .flat_map(str::split_whitespace)
}

/// The hyphenators of several languages, by language tag. Cloning shares
/// them.
#[derive(Clone, Debug, Default)]
pub struct Hyphenation {
    by_tag: BTreeMap<String, Arc<Hyphenator>>,
}

impl Hyphenation {
    /// None.
    pub fn new() -> Hyphenation {
        Hyphenation::default()
    }

    /// Hyphenates `language` with `hyphenator`, as text in that language
    /// or a more specific one asks: `en` serves `en-AU` unless `en-AU` has
    /// its own.
    pub fn insert(&mut self, language: &Language, hyphenator: Arc<Hyphenator>) {
        self.by_tag.insert(language.to_string(), hyphenator);
    }

    /// The hyphenator for `language`: its own, or its language and region's,
    /// or its language's.
    pub fn get(&self, language: &Language) -> Option<&Hyphenator> {
        let id = language.identifier();
        let base = id.language.as_str();
        let regional = id.region.map(|r| format!("{base}-{}", r.as_str()));
        [Some(language.to_string()), regional, Some(base.to_string())]
            .into_iter()
            .flatten()
            .find_map(|tag| self.by_tag.get(&tag))
            .map(Arc::as_ref)
    }

    /// Whether `language` has a hyphenator here.
    pub fn has(&self, language: &Language) -> bool {
        self.get(language).is_some()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.by_tag.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few of TeX's English patterns, enough for "hyphenation".
    const PATTERNS: &str = "hy3ph he2n hena4 hen5at 1na n2at 1tio 2io o2n";

    fn split(h: &Hyphenator, word: &str) -> String {
        let mut out = String::new();
        let mut last = 0;
        for b in h.hyphenate(word) {
            out.push_str(&word[last..b]);
            out.push('-');
            last = b;
        }
        out.push_str(&word[last..]);
        out
    }

    #[test]
    fn liangs_example_hyphenates() {
        let h = Hyphenator::parse(PATTERNS, "", 2, 3);
        assert_eq!(split(&h, "hyphenation"), "hy-phen-ation");
        // Case doesn't matter, and offsets are bytes of the word as given.
        assert_eq!(split(&h, "HYPHENATION"), "HY-PHEN-ATION");
    }

    #[test]
    fn minimums_keep_letters_on_each_side() {
        let h = Hyphenator::parse(PATTERNS, "", 3, 5);
        assert_eq!(split(&h, "hyphenation"), "hyphen-ation");
        let h = Hyphenator::parse("a1b", "", 2, 2);
        assert_eq!(split(&h, "ab"), "ab");
        assert_eq!(split(&h, "xab"), "xab");
        assert_eq!(split(&h, "xxabxx"), "xxa-bxx");
    }

    #[test]
    fn exceptions_win() {
        let h = Hyphenator::parse(PATTERNS, "hy-phen-a-tion\npresent", 2, 2);
        assert_eq!(split(&h, "Hyphenation"), "Hy-phen-a-tion");
        let h = Hyphenator::parse("e1s", "present", 2, 2);
        assert_eq!(split(&h, "present"), "present");
    }

    #[test]
    fn letters_beyond_ascii_and_comments() {
        let h = Hyphenator::parse("% a comment 1x\nö1b", "", 1, 1);
        assert_eq!(split(&h, "Öböb"), "Ö-bö-b");
    }

    #[test]
    fn languages_fall_back_to_their_base() {
        let mut all = Hyphenation::new();
        let en = Arc::new(Hyphenator::parse(PATTERNS, "", 2, 3));
        let gb = Arc::new(Hyphenator::parse("", "", 2, 3));
        all.insert(&Language::parse("en").unwrap(), en);
        all.insert(&Language::parse("en-GB").unwrap(), gb);
        let get = |tag: &str| {
            all.get(&Language::parse(tag).unwrap())
                .map(|h| h.nodes.len())
        };
        assert!(get("en-AU").unwrap() > 1);
        assert_eq!(get("en-GB"), Some(1));
        assert_eq!(get("en-Latn-GB"), Some(1));
        assert!(get("en").unwrap() > 1);
        assert_eq!(get("de"), None);
    }
}
