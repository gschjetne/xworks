//! Text as it's typed or pasted, in the bundled fonts: any UTF-8 lays out
//! without a panic, its lines cover it and fit, and the caret can visit
//! every place in it. The first bytes choose the width, direction and
//! alignment; the rest is the text. Each U+FFFC in it stands for an inline
//! object, as a picture does, each its own size, and each is placed once.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use text::{
    Affinity, Align, Cursor, Direction, Fonts, Inline, Paragraph, Setting, Span, Style, units,
};

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Fonts::new();
        for (_, bytes) in
            testkit::fonts::load().expect("the bundled fonts; run cargo run -p fetch-fonts")
        {
            fonts.add(bytes).expect("a bundled font");
        }
        let spec: text::FontSpec =
            ron::from_str(&testkit::fonts::spec().expect("spec/text/fonts.ron")).expect("valid");
        fonts.set_fallbacks(&spec);
        fonts
    })
}

fuzz_target!(|data: &[u8]| {
    let [width, settings, rest @ ..] = data else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let width = i64::from(*width) * units::POINT;
    let direction = match settings % 3 {
        0 => None,
        1 => Some(Direction::Ltr),
        _ => Some(Direction::Rtl),
    };
    let align = match (settings / 3) % 4 {
        0 => Align::Start,
        1 => Align::Center,
        2 => Align::End,
        _ => Align::Justify,
    };
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: if settings & 0x80 == 0 {
                "Noto Serif"
            } else {
                "Noto Sans"
            }
            .to_string(),
            size: 12 * units::POINT,
            weight: if settings & 0x40 == 0 { 400 } else { 700 },
            italic: settings & 0x20 != 0,
            language: text::Language::und(),
            ligatures: text::Ligatures::default(),
        },
    }];
    // Objects from a point to a page wide, some taller than a line.
    let objects: Vec<Inline> = text
        .match_indices('\u{fffc}')
        .enumerate()
        .map(|(k, (offset, _))| Inline {
            offset,
            width: (k as i64 % 7) * 50 * units::POINT + units::POINT,
            height: (k as i64 % 5) * 20 * units::POINT,
        })
        .collect();
    let layout = text::layout_with(
        fonts(),
        &Paragraph {
            text,
            spans: &spans,
            direction,
            align,
            width,
        },
        &Setting {
            objects: &objects,
            ..Setting::default()
        },
    );
    let placed: usize = layout.lines.iter().map(|l| l.objects.len()).sum();
    assert_eq!(placed, objects.len());
    let mut at = 0;
    for line in &layout.lines {
        assert_eq!(line.range.start, at);
        let graphemes = locale::graphemes(&text[line.range.clone()]).len() - 1;
        assert!(line.width <= width || graphemes <= 1);
        at = line.range.end;
    }
    assert_eq!(at, text.len());
    let mut cursor = Some(Cursor {
        offset: 0,
        affinity: Affinity::Downstream,
    });
    while let Some(c) = cursor {
        let caret = layout.caret(c);
        let hit = layout.caret(layout.hit(caret.x, caret.top));
        assert_eq!((hit.x, hit.line), (caret.x, caret.line));
        let _ = (layout.left(c), layout.right(c));
        cursor = layout.next(c);
    }
});
