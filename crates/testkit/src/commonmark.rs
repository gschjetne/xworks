//! CommonMark's own examples, listed in `spec/markdown/commonmark.ron` and
//! kept in `commonmark/`, beside the fonts, where `cargo run -p
//! fetch-fonts` puts them.

use std::path::PathBuf;

use serde::Deserialize;

use crate::Error;
use crate::fonts::FontFile;

/// What `spec/markdown/commonmark.ron` lists.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The spec's examples' file, and where it comes from.
    pub files: Vec<FontFile>,
}

/// One of the spec's examples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    /// Its number in the spec.
    pub number: u32,
    /// The section it's in.
    pub section: String,
    /// Its Markdown.
    pub markdown: String,
    /// The HTML the Markdown means.
    pub html: String,
}

/// The path of `spec/markdown/commonmark.ron`.
pub fn spec_path() -> PathBuf {
    crate::workspace_root().join("spec/markdown/commonmark.ron")
}

/// The directory the examples are kept in.
pub fn dir() -> PathBuf {
    crate::workspace_root().join("commonmark")
}

/// What `spec/markdown/commonmark.ron` lists.
pub fn manifest() -> Result<Manifest, Error> {
    let path = spec_path();
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    ron::from_str(&text).map_err(|e| Error::Record {
        path,
        source: Box::new(e),
    })
}

/// Every example, in the spec's order. A missing file is an error that
/// says how to fetch it.
pub fn load() -> Result<Vec<Example>, Error> {
    let mut out = Vec::new();
    for file in manifest()?.files {
        let path = dir().join(&file.file);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::MissingFetched { path });
            }
            Err(source) => return Err(Error::Io { path, source }),
        };
        let value = Json::parse(&text).ok_or_else(|| Error::Json { path: path.clone() })?;
        let Json::Array(items) = value else {
            return Err(Error::Json { path });
        };
        for item in items {
            let field = |name: &str| match &item {
                Json::Object(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
                _ => None,
            };
            let text = |name: &str| match field(name) {
                Some(Json::String(s)) => Some(s.clone()),
                _ => None,
            };
            let (Some(markdown), Some(html), Some(section), Some(Json::Number(number))) = (
                text("markdown"),
                text("html"),
                text("section"),
                field("example"),
            ) else {
                return Err(Error::Json { path });
            };
            out.push(Example {
                number: *number as u32,
                section,
                markdown,
                html,
            });
        }
    }
    Ok(out)
}

/// JSON, as much of it as the spec's examples are: arrays of objects of
/// strings and numbers.
#[derive(Debug, Clone, PartialEq)]
enum Json {
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
    String(String),
    Number(f64),
    Other,
}

impl Json {
    /// `text` as JSON, if it's JSON.
    fn parse(text: &str) -> Option<Json> {
        let chars: Vec<char> = text.chars().collect();
        let mut at = 0;
        let value = value(&chars, &mut at)?;
        skip(&chars, &mut at);
        (at == chars.len()).then_some(value)
    }
}

fn skip(chars: &[char], at: &mut usize) {
    while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
        *at += 1;
    }
}

/// The four hexadecimal digits of a `\u` escape at `at`, as a number.
fn unit(chars: &[char], at: &mut usize) -> Option<u32> {
    let hex: String = chars.get(*at..*at + 4)?.iter().collect();
    *at += 4;
    u32::from_str_radix(&hex, 16).ok()
}

fn value(chars: &[char], at: &mut usize) -> Option<Json> {
    skip(chars, at);
    match chars.get(*at)? {
        '[' => {
            *at += 1;
            let mut items = Vec::new();
            loop {
                skip(chars, at);
                if chars.get(*at) == Some(&']') {
                    *at += 1;
                    return Some(Json::Array(items));
                }
                items.push(value(chars, at)?);
                skip(chars, at);
                match chars.get(*at)? {
                    ',' => *at += 1,
                    ']' => {
                        *at += 1;
                        return Some(Json::Array(items));
                    }
                    _ => return None,
                }
            }
        }
        '{' => {
            *at += 1;
            let mut fields = Vec::new();
            loop {
                skip(chars, at);
                if chars.get(*at) == Some(&'}') {
                    *at += 1;
                    return Some(Json::Object(fields));
                }
                let Json::String(key) = value(chars, at)? else {
                    return None;
                };
                skip(chars, at);
                if chars.get(*at) != Some(&':') {
                    return None;
                }
                *at += 1;
                fields.push((key, value(chars, at)?));
                skip(chars, at);
                match chars.get(*at)? {
                    ',' => *at += 1,
                    '}' => {
                        *at += 1;
                        return Some(Json::Object(fields));
                    }
                    _ => return None,
                }
            }
        }
        '"' => {
            *at += 1;
            let mut s = String::new();
            loop {
                let c = *chars.get(*at)?;
                *at += 1;
                match c {
                    '"' => return Some(Json::String(s)),
                    '\\' => {
                        let e = *chars.get(*at)?;
                        *at += 1;
                        match e {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            'b' => s.push('\u{8}'),
                            'f' => s.push('\u{c}'),
                            'u' => {
                                let high = unit(chars, at)?;
                                let code = if (0xd800..0xdc00).contains(&high) {
                                    if chars.get(*at..*at + 2)? != ['\\', 'u'] {
                                        return None;
                                    }
                                    *at += 2;
                                    let low = unit(chars, at)?;
                                    0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00)
                                } else {
                                    high
                                };
                                s.push(char::from_u32(code)?);
                            }
                            other => s.push(other),
                        }
                    }
                    c => s.push(c),
                }
            }
        }
        c if *c == '-' || c.is_ascii_digit() => {
            let start = *at;
            while chars
                .get(*at)
                .is_some_and(|c| c.is_ascii_digit() || "+-.eE".contains(*c))
            {
                *at += 1;
            }
            let number: String = chars[start..*at].iter().collect();
            number.parse().ok().map(Json::Number)
        }
        _ => {
            for word in ["true", "false", "null"] {
                let w: Vec<char> = word.chars().collect();
                if chars.get(*at..*at + w.len()) == Some(&w[..]) {
                    *at += w.len();
                    return Some(Json::Other);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_reads_as_the_examples_need() {
        let text = r#"[{"markdown": "a\tb\né😀", "example": 12, "ok": true}]"#;
        let Some(Json::Array(items)) = Json::parse(text) else {
            panic!("an array");
        };
        assert_eq!(
            items[0],
            Json::Object(vec![
                ("markdown".into(), Json::String("a\tb\né😀".into())),
                ("example".into(), Json::Number(12.0)),
                ("ok".into(), Json::Other),
            ])
        );
        assert_eq!(Json::parse("[1,"), None);
    }
}
