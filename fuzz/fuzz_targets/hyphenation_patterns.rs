//! Hyphenation pattern and exception files, as TeX ships them: any text
//! reads as patterns without a panic, and words hyphenate at character
//! boundaries inside them, keeping the minimums. The first bytes choose
//! the minimums and where the exceptions and the word to hyphenate start.

#![no_main]

use libfuzzer_sys::fuzz_target;
use text::Hyphenator;

fuzz_target!(|data: &[u8]| {
    let [left, right, split, rest @ ..] = data else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let (left, right) = (usize::from(*left % 4), usize::from(*right % 4));
    let split = text
        .char_indices()
        .map(|(i, _)| i)
        .nth(usize::from(*split))
        .unwrap_or(text.len());
    let (patterns, exceptions) = text.split_at(split);
    let hyphenator = Hyphenator::parse(patterns, exceptions, left, right);
    for word in patterns.split_whitespace().chain(exceptions.split_whitespace()) {
        let n = word.chars().count();
        let mut previous = 0;
        for b in hyphenator.hyphenate(word) {
            assert!(word.is_char_boundary(b) && b > previous && b < word.len());
            let before = word[..b].chars().count();
            assert!(before >= left.max(1) && n - before >= right.max(1));
            previous = b;
        }
    }
});
