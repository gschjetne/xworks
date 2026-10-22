//! Words hyphenated in every language with patterns
//! (`spec/text/hyphenated.ron`), and the pattern reader on any text.

mod common;

use common::{hyphenation, parse};
use proptest::prelude::*;
use text::{Hyphenator, Language};

#[test]
fn words_break_where_their_languages_allow() {
    let source =
        std::fs::read_to_string(testkit::workspace_root().join("spec/text/hyphenated.ron"))
            .expect("spec/text/hyphenated.ron");
    let cases: Vec<(String, Vec<String>)> = parse(&source).expect("valid");
    let mut failures = Vec::new();
    for (tag, words) in cases {
        let language = Language::parse(&tag).expect("a language tag");
        let hyphenator = hyphenation()
            .get(&language)
            .unwrap_or_else(|| panic!("patterns for {tag}"));
        for want in words {
            let word = want.replace('-', "");
            let mut got = String::new();
            let mut last = 0;
            for b in hyphenator.hyphenate(&word) {
                got.push_str(&word[last..b]);
                got.push('-');
                last = b;
            }
            got.push_str(&word[last..]);
            if got != want {
                failures.push(format!("{tag}: {got}, expected {want}"));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn every_language_in_the_spec_has_patterns() {
    let manifest = testkit::hyphenation::manifest().expect("spec/text/hyphenation.ron");
    for language in manifest.languages {
        for tag in &language.tags {
            let tag = Language::parse(tag).expect("a language tag");
            assert!(hyphenation().has(&tag), "{}: {tag}", language.name);
        }
        // Every file it names is listed, to be fetched.
        let mut names = vec![language.patterns.clone()];
        names.extend(language.exceptions.clone());
        names.extend(language.notices.clone());
        for name in names {
            assert!(
                manifest.files.iter().any(|f| f.file == name),
                "{}: {name} isn't listed",
                language.name
            );
        }
    }
}

proptest! {
    /// Any text reads as patterns and exceptions, and any word hyphenates
    /// at character boundaries inside it, leaving the minimums.
    #[test]
    fn any_patterns_read_and_hyphenate(
        patterns in "[a-c1-9.%\n ]{0,60}",
        exceptions in "[a-c\\- \n]{0,30}",
        word in "[a-cA-Cé]{0,12}",
        left in 1usize..4,
        right in 1usize..4,
    ) {
        let h = Hyphenator::parse(&patterns, &exceptions, left, right);
        let breaks = h.hyphenate(&word);
        let n = word.chars().count();
        let mut previous = 0;
        for b in breaks {
            prop_assert!(word.is_char_boundary(b) && b > previous && b < word.len());
            let before = word[..b].chars().count();
            prop_assert!(before >= left && n - before >= right);
            previous = b;
        }
    }
}
