//! The language tag reaches the shaper (spec/text/languages.ron): the same
//! characters take each language's forms from the font.

mod common;

use common::{StyleSpec, fonts, parse};
use serde::Deserialize;
use text::{Align, Paragraph, Span};

#[derive(Debug, Deserialize)]
struct Entry {
    about: String,
    text: String,
    style: StyleSpec,
    glyphs: Vec<(String, Vec<u32>)>,
}

#[test]
fn languages_pick_their_forms() {
    let source = testkit::specs("text")
        .expect("spec/text")
        .into_iter()
        .find(|(name, _)| name == "languages")
        .expect("spec/text/languages.ron")
        .1;
    let entries: Vec<Entry> = parse(&source).expect("languages.ron");
    let mut failures = Vec::new();
    for entry in &entries {
        for (language, want) in &entry.glyphs {
            let style = StyleSpec {
                language: Some(language.clone()),
                ..entry.style.clone()
            }
            .style();
            let spans = [Span {
                len: entry.text.len(),
                style,
            }];
            let layout = text::layout(
                fonts(),
                &Paragraph {
                    text: &entry.text,
                    spans: &spans,
                    direction: None,
                    align: Align::Start,
                    width: i64::MAX / 4,
                },
            );
            // In the text's order, whichever way it runs.
            let mut glyphs: Vec<(usize, u32)> = layout
                .lines
                .iter()
                .flat_map(|l| &l.runs)
                .flat_map(|r| r.glyphs.iter().map(|g| (g.cluster, g.id)))
                .collect();
            glyphs.sort_by_key(|&(cluster, _)| cluster);
            let got: Vec<u32> = glyphs.into_iter().map(|(_, id)| id).collect();
            if &got != want {
                failures.push(format!(
                    "{} ({language}): {got:?}, expected {want:?}",
                    entry.about
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
