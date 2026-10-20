use std::ops::Range;

use icu_properties::CodePointMapData;
use icu_properties::props::Script as IcuScript;

/// A writing system, as its ISO 15924 code: `Latn`, `Grek`, `Cyrl`,
/// `Arab`, `Hebr`, `Hani`, `Zyyy` for common characters, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Script([u8; 4]);

impl Script {
    /// Characters every script uses: spaces, digits, most punctuation.
    pub const COMMON: Script = Script(*b"Zyyy");

    /// Its four-letter code.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("ISO 15924 codes are ASCII")
    }

    /// Its four-letter code, as bytes.
    pub fn code(&self) -> [u8; 4] {
        self.0
    }

    fn of(c: char) -> Option<Script> {
        let script = CodePointMapData::<IcuScript>::new().get(c);
        if matches!(
            script,
            IcuScript::Common | IcuScript::Inherited | IcuScript::Unknown
        ) {
            return None;
        }
        let tag = icu_locale_core::subtags::Script::from(script);
        tag.as_str().as_bytes().try_into().ok().map(Script)
    }
}

impl std::fmt::Display for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `text` divided into runs of one script each, in order, covering it.
///
/// Characters common to many scripts, such as spaces, digits and
/// punctuation, and combining marks belong to the script before them, or
/// at the start of the text to the first script after them. Text with no
/// script of its own is one [`Script::COMMON`] run.
pub fn script_runs(text: &str) -> Vec<(Range<usize>, Script)> {
    let mut runs: Vec<(Range<usize>, Script)> = Vec::new();
    // Characters before the first with a script of its own.
    let mut lead = 0;
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        match (Script::of(c), runs.last_mut()) {
            (None, Some(last)) => last.0.end = end,
            (None, None) => lead = end,
            (Some(s), Some(last)) if last.1 == s => last.0.end = end,
            (Some(s), Some(_)) => runs.push((i..end, s)),
            (Some(s), None) => runs.push((0..end, s)),
        }
    }
    if runs.is_empty() && lead > 0 {
        runs.push((0..lead, Script::COMMON));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str) -> Vec<(&str, String)> {
        script_runs(text)
            .into_iter()
            .map(|(r, s)| (&text[r], s.to_string()))
            .collect()
    }

    #[test]
    fn scripts_are_separated_and_common_characters_join_the_one_before() {
        assert_eq!(
            runs("Hi Γειά, Привет! שלום مرحبا 日本"),
            [
                ("Hi ", "Latn".to_string()),
                ("Γειά, ", "Grek".to_string()),
                ("Привет! ", "Cyrl".to_string()),
                ("שלום ", "Hebr".to_string()),
                ("مرحبا ", "Arab".to_string()),
                ("日本", "Hani".to_string()),
            ]
        );
    }

    #[test]
    fn leading_common_characters_join_the_first_script() {
        assert_eq!(runs("«Ελλάδα»"), [("«Ελλάδα»", "Grek".to_string())]);
        assert_eq!(runs("123 ..."), [("123 ...", "Zyyy".to_string())]);
        assert_eq!(runs(""), []);
    }

    #[test]
    fn combining_marks_stay_with_their_base() {
        assert_eq!(
            runs("e\u{301}λ"),
            [("e\u{301}", "Latn".to_string()), ("λ", "Grek".to_string())]
        );
    }
}
