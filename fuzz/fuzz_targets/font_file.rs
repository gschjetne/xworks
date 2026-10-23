//! Font files, as a document embeds them or a user installs them: any bytes
//! load as faces or give an error, never a panic, and text laid out in a
//! face that loads can be measured and edited.

#![no_main]

use libfuzzer_sys::fuzz_target;
use text::{Affinity, Align, Cursor, Fonts, Paragraph, Span, Style, units};

fuzz_target!(|data: &[u8]| {
    let mut fonts = Fonts::new();
    let Ok(faces) = fonts.add(data.to_vec()) else {
        return;
    };
    for id in faces {
        let face = fonts.face(id);
        let text = "Hello, world! Γειά σου κόσμε. שלום עולם. مرحبا. 日本語。";
        let spans = [Span {
            len: text.len(),
            style: Style {
                family: face.family().to_string(),
                size: 12 * units::POINT,
                weight: face.weight(),
                italic: face.italic(),
                language: text::Language::und(),
                ligatures: text::Ligatures::default(),
            },
        }];
        let layout = text::layout(
            &fonts,
            &Paragraph {
                text,
                spans: &spans,
                direction: None,
                align: Align::Start,
                width: 100 * units::POINT,
            },
        );
        let mut cursor = Some(Cursor {
            offset: 0,
            affinity: Affinity::Downstream,
        });
        while let Some(c) = cursor {
            let caret = layout.caret(c);
            let _ = layout.hit(caret.x, caret.top);
            let _ = layout.right(c);
            cursor = layout.next(c);
        }
    }
});
