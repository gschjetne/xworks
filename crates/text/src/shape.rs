//! Itemizing and shaping: a paragraph divided into stretches of one style,
//! one face, one bidi level and one script, each shaped with HarfRust.

use std::ops::Range;
use std::sync::{Arc, PoisonError, RwLock};

use harfrust::{Buffer, BufferFlags, ShapeOptions, ShapePlan, ShaperFont, Tag};
use locale::{Bidi, Language, Script};

use crate::font::{Face, FaceId, Fonts};
use crate::layout::{Inline, Ligatures, Span};
use crate::units::scale;

/// What a shaping plan is made for, in its face: which way the text runs,
/// its script and language, and the ligatures it's set with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanKey {
    pub rtl: bool,
    pub script: Script,
    pub language: Language,
    pub ligatures: Ligatures,
}

/// A plan HarfRust shapes with, and the language as it takes it.
#[derive(Clone)]
pub(crate) struct Plan {
    pub plan: Arc<ShapePlan>,
    pub language: Option<harfrust::Language>,
}

/// The plans a face has shaped with, each made once for its key and kept:
/// HarfRust would otherwise make one, choosing the font's features and
/// lookups again, for every stretch of text it shapes. A plan depends on
/// nothing but its face and its key, so text shapes the same with a plan
/// kept as with one made afresh. Clones of a face share them, as they share
/// its data.
#[derive(Clone, Default)]
pub(crate) struct Plans(Arc<RwLock<Vec<(PlanKey, Plan)>>>);

impl Plans {
    /// The plan for `key` in `font`, made if there's none yet.
    pub(crate) fn get(&self, font: &harfrust::Font, key: &PlanKey) -> Plan {
        let found = |plans: &[(PlanKey, Plan)]| {
            plans
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, plan)| plan.clone())
        };
        if let Some(plan) = found(&self.0.read().unwrap_or_else(PoisonError::into_inner)) {
            return plan;
        }
        let language = (key.language != Language::und())
            .then(|| harfrust::Language::new(key.language.to_string()))
            .flatten();
        let plan = Plan {
            plan: Arc::new(ShapePlan::new(
                font,
                direction(key.rtl),
                harfrust_script(key.script),
                language.as_ref(),
                &key.ligatures.features(),
            )),
            language,
        };
        let mut plans = self.0.write().unwrap_or_else(PoisonError::into_inner);
        // Another layout may have made it meanwhile: the same plan, as
        // plans depend only on their keys.
        if let Some(plan) = found(&plans) {
            return plan;
        }
        plans.push((key.clone(), plan.clone()));
        plan
    }

    /// How many plans are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.read().unwrap_or_else(PoisonError::into_inner).len()
    }
}

fn direction(rtl: bool) -> harfrust::Direction {
    if rtl {
        harfrust::Direction::RightToLeft
    } else {
        harfrust::Direction::LeftToRight
    }
}

fn harfrust_script(script: Script) -> Option<harfrust::Script> {
    harfrust::Script::from_iso15924_tag(Tag::new(&script.code()))
}

/// A stretch of a paragraph shaped in one go.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    /// Its text, as byte offsets into the paragraph.
    pub range: Range<usize>,
    /// The span whose style it has.
    pub span: usize,
    pub face: FaceId,
    pub level: u8,
    pub script: Script,
    /// Whether a character in it has no face, so it shows as a box.
    pub missing: bool,
    /// Whether it's an inline object's character, which is an item of its
    /// own, so no glyph run holds both an object and text.
    pub object: bool,
    /// Its glyphs, left to right.
    pub glyphs: Vec<Shaped>,
}

/// A shaped glyph, in layout units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shaped {
    pub id: u32,
    /// Where its cluster starts, as a byte offset into the paragraph.
    pub cluster: usize,
    pub advance: i64,
    pub x_offset: i64,
    /// Up from the baseline, as fonts measure it.
    pub y_offset: i64,
    /// Whether breaking the line at its cluster changes the shaping, so
    /// the text either side must be shaped again.
    pub unsafe_to_break: bool,
    /// A hard line break, which is never drawn.
    pub hidden: bool,
    /// A hyphen layout added at a line's end, which shows no character
    /// of the text.
    pub inserted: bool,
    /// An inline object, as wide as its advance, which no font draws.
    pub object: bool,
}

/// The inline object at byte `offset`, if there's one: `objects` are
/// sorted by offset.
pub(crate) fn object_at(objects: &[Inline], offset: usize) -> Option<&Inline> {
    objects
        .binary_search_by_key(&offset, |o| o.offset)
        .ok()
        .map(|i| &objects[i])
}

/// `text` divided into items and shaped. `spans` cover the text; items
/// start and end on grapheme boundaries (`graphemes`). Each grapheme that
/// starts with one of `objects` is an item of its own.
pub(crate) fn itemize(
    fonts: &Fonts,
    text: &str,
    spans: &[Span],
    bidi: &Bidi<'_>,
    graphemes: &[usize],
    objects: &[Inline],
) -> Vec<Item> {
    let scripts = locale::script_runs(text);
    let candidates: Vec<Vec<FaceId>> = spans
        .iter()
        .map(|s| fonts.candidates(&s.style.family, s.style.weight, s.style.italic))
        .collect();
    let mut items: Vec<Item> = Vec::new();
    let (mut span, mut span_end) = (0, spans.first().map_or(0, |s| s.len));
    let mut script = 0;
    for pair in graphemes.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        while start >= span_end && span + 1 < spans.len() {
            span += 1;
            span_end += spans[span].len;
        }
        while scripts[script].0.end <= start {
            script += 1;
        }
        let cluster = &text[start..end];
        let previous = items
            .last()
            .filter(|item| item.span == span && item.range.end == start)
            .map(|item| item.face);
        let object = object_at(objects, start).is_some();
        // An object needs no font: it takes the face of the text around
        // it, which sets its line's least height.
        let (face, found) = if object {
            (
                previous
                    .unwrap_or_else(|| *candidates[span].first().expect("fonts to lay out with")),
                true,
            )
        } else {
            choose(fonts, cluster, &candidates[span], previous)
        };
        let level = bidi.level(start);
        let script = scripts[script].1;
        match items.last_mut() {
            Some(item)
                if item.span == span
                    && item.face == face
                    && item.level == level
                    && item.script == script
                    && !item.object
                    && !object =>
            {
                item.range.end = end;
                item.missing |= !found;
            }
            _ => items.push(Item {
                range: start..end,
                span,
                face,
                level,
                script,
                missing: !found,
                object,
                glyphs: Vec::new(),
            }),
        }
    }
    for item in &mut items {
        item.glyphs = shape(
            fonts,
            text,
            item.range.clone(),
            &spans[item.span],
            item.face,
            item.level,
            item.script,
            objects,
        );
    }
    items
}

/// The face for the grapheme `cluster`, and whether it has the cluster:
/// the one before it if the cluster has no script of its own and that face
/// has it, so spaces and punctuation stay with the text around them;
/// otherwise the first candidate that has every character; otherwise the
/// first candidate, which hasn't.
fn choose(
    fonts: &Fonts,
    cluster: &str,
    candidates: &[FaceId],
    previous: Option<FaceId>,
) -> (FaceId, bool) {
    let covers = |id: FaceId| covers(fonts.face(id), cluster);
    if let Some(previous) = previous
        && cluster.chars().all(|c| Script::of(c).is_none())
        && covers(previous)
    {
        return (previous, true);
    }
    match candidates.iter().copied().find(|&id| covers(id)) {
        Some(id) => (id, true),
        None => (*candidates.first().expect("fonts to lay out with"), false),
    }
}

/// Whether `face` has every character of `cluster` that's drawn.
fn covers(face: &Face, cluster: &str) -> bool {
    cluster.chars().all(|c| is_invisible(c) || face.has(c))
}

/// Characters drawn with no glyph of their own, whatever the font: format
/// and control characters, variation selectors and hard line breaks.
fn is_invisible(c: char) -> bool {
    is_hard_break(c)
        || c.is_control()
        || matches!(c,
            '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{180b}'..='\u{180f}'
            | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}'
            | '\u{e0000}'..='\u{e0fff}')
}

/// Whether a line must break after `c`.
pub(crate) fn is_hard_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// The glyphs of `range` of `text`, in `face` at `span`'s size, at bidi
/// `level`, left to right. The text around the range is the shaper's
/// context, so letters join across it. Each of `objects` in the range is
/// a glyph of its own, as wide as the object, which no font draws.
#[allow(
    clippy::too_many_arguments,
    reason = "what's shaped, and how, as itemizing found it"
)]
pub(crate) fn shape(
    fonts: &Fonts,
    text: &str,
    range: Range<usize>,
    span: &Span,
    face: FaceId,
    level: u8,
    script: Script,
    objects: &[Inline],
) -> Vec<Shaped> {
    let first = objects.partition_point(|o| o.offset < range.start);
    let last = objects.partition_point(|o| o.offset < range.end);
    let inside = &objects[first..last.max(first)];
    if inside.is_empty() {
        return shape_text(fonts, text, range, span, face, level, script);
    }
    // The text between the objects shaped as text, and each object a
    // glyph; right to left, the pieces go the other way.
    let mut pieces: Vec<Vec<Shaped>> = Vec::with_capacity(inside.len() * 2 + 1);
    let mut from = range.start;
    for object in inside {
        if object.offset > from {
            pieces.push(shape_text(
                fonts,
                text,
                from..object.offset,
                span,
                face,
                level,
                script,
            ));
        }
        pieces.push(vec![Shaped {
            id: 0,
            cluster: object.offset,
            advance: object.width,
            x_offset: 0,
            y_offset: 0,
            unsafe_to_break: false,
            hidden: false,
            inserted: false,
            object: true,
        }]);
        from = object.offset
            + text[object.offset..]
                .chars()
                .next()
                .map_or(1, char::len_utf8);
    }
    if from < range.end {
        pieces.push(shape_text(
            fonts,
            text,
            from..range.end,
            span,
            face,
            level,
            script,
        ));
    }
    if level % 2 == 1 {
        pieces.reverse();
    }
    pieces.into_iter().flatten().collect()
}

/// The glyphs of `range` of `text`, which holds no objects, as [`shape`]
/// gives them.
fn shape_text(
    fonts: &Fonts,
    text: &str,
    range: Range<usize>,
    span: &Span,
    face: FaceId,
    level: u8,
    script: Script,
) -> Vec<Shaped> {
    let face = fonts.face(face);
    let shaper = ShaperFont::new(&face.font);
    let key = PlanKey {
        rtl: level % 2 == 1,
        script,
        language: span.style.language.clone(),
        ligatures: span.style.ligatures,
    };
    let plan = face.plans.get(&face.font, &key);
    let mut buffer = Buffer::new();
    buffer.reserve(range.len());
    for (i, c) in text[range.clone()].char_indices() {
        // A tab is shaped as a space, whose glyph every font has and which
        // draws nothing; layout makes it as wide as its stop asks.
        let c = if c == '\t' { ' ' } else { c };
        buffer.push(u32::from(c), (range.start + i) as u32);
    }
    buffer.set_pre_context(&text[..range.start]);
    buffer.set_post_context(&text[range.end..]);
    buffer.set_direction(direction(key.rtl));
    buffer.set_script(harfrust_script(script));
    if plan.language.is_some() {
        buffer.set_language(plan.language.clone());
    }
    let mut flags = BufferFlags::empty();
    if range.start == 0 {
        flags |= BufferFlags::BEGINNING_OF_TEXT;
    }
    if range.end == text.len() {
        flags |= BufferFlags::END_OF_TEXT;
    }
    buffer.set_flags(flags);
    let features = span.style.ligatures.features();
    if harfrust::shape(
        &shaper,
        &mut buffer,
        ShapeOptions::new()
            .plan(Some(&*plan.plan))
            .features(&features),
    )
    .is_err()
    {
        // The buffer's direction and script are always set, so shaping
        // can't fail; if it ever does, the text takes no room.
        return Vec::new();
    }
    let size = span.style.size;
    let upem = face.units_per_em;
    buffer
        .glyph_infos()
        .iter()
        .zip(buffer.glyph_positions())
        .map(|(info, pos)| {
            let cluster = info.cluster as usize;
            let hidden = text[cluster..].chars().next().is_some_and(is_hard_break);
            Shaped {
                id: info.glyph_id,
                cluster,
                advance: if hidden {
                    0
                } else {
                    scale(pos.x_advance, size, upem)
                },
                x_offset: scale(pos.x_offset, size, upem),
                y_offset: scale(pos.y_offset, size, upem),
                unsafe_to_break: info.unsafe_to_break(),
                hidden,
                inserted: false,
                object: false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Style;
    use crate::units::POINT;

    /// The bundled fonts, but those kept until text needs them.
    fn fonts() -> Fonts {
        let mut fonts = Fonts::new();
        for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
            if !name.contains("CJK") {
                fonts.add(bytes).expect("a font");
            }
        }
        fonts
    }

    /// `range` of `text` shaped as [`shape_text`] shaped it before plans
    /// were kept: HarfRust making a plan for the call.
    fn afresh(
        fonts: &Fonts,
        text: &str,
        range: Range<usize>,
        span: &Span,
        face: FaceId,
        level: u8,
        script: Script,
    ) -> Vec<Shaped> {
        let face = fonts.face(face);
        let shaper = ShaperFont::new(&face.font);
        let mut buffer = Buffer::new();
        for (i, c) in text[range.clone()].char_indices() {
            let c = if c == '\t' { ' ' } else { c };
            buffer.push(u32::from(c), (range.start + i) as u32);
        }
        buffer.set_pre_context(&text[..range.start]);
        buffer.set_post_context(&text[range.end..]);
        buffer.set_direction(direction(level % 2 == 1));
        buffer.set_script(harfrust_script(script));
        if span.style.language != Language::und() {
            buffer.set_language(harfrust::Language::new(span.style.language.to_string()));
        }
        let mut flags = BufferFlags::empty();
        if range.start == 0 {
            flags |= BufferFlags::BEGINNING_OF_TEXT;
        }
        if range.end == text.len() {
            flags |= BufferFlags::END_OF_TEXT;
        }
        buffer.set_flags(flags);
        let features = span.style.ligatures.features();
        harfrust::shape(
            &shaper,
            &mut buffer,
            ShapeOptions::new().features(&features),
        )
        .expect("shaped");
        let (size, upem) = (span.style.size, face.units_per_em);
        buffer
            .glyph_infos()
            .iter()
            .zip(buffer.glyph_positions())
            .map(|(info, pos)| {
                let cluster = info.cluster as usize;
                let hidden = text[cluster..].chars().next().is_some_and(is_hard_break);
                Shaped {
                    id: info.glyph_id,
                    cluster,
                    advance: if hidden {
                        0
                    } else {
                        scale(pos.x_advance, size, upem)
                    },
                    x_offset: scale(pos.x_offset, size, upem),
                    y_offset: scale(pos.y_offset, size, upem),
                    unsafe_to_break: info.unsafe_to_break(),
                    hidden,
                    inserted: false,
                    object: false,
                }
            })
            .collect()
    }

    /// Text shaped with a plan kept, the first time and every time after,
    /// gets the glyphs HarfRust gives with a plan made for the call: in
    /// each script and direction, with a language and without, with
    /// ligatures off, which take a plan of their own, and part of a
    /// paragraph, with the text around it as context.
    #[test]
    fn a_kept_plan_shapes_as_one_made_afresh() {
        let fonts = fonts();
        let none = Ligatures {
            standard: false,
            contextual: false,
            discretionary: false,
            historical: false,
        };
        let all = Ligatures {
            standard: true,
            contextual: true,
            discretionary: true,
            historical: true,
        };
        let on = Ligatures::default();
        let cases = [
            (
                "Noto Serif",
                "The office's affluent fjord, AV To.",
                "und",
                on,
                0,
            ),
            (
                "EB Garamond",
                "Office stuff, fifty-fifth\tcafé",
                "en-GB",
                on,
                0,
            ),
            (
                "EB Garamond",
                "Office stuff, fifty-fifth\tcafé",
                "en-GB",
                none,
                0,
            ),
            (
                "EB Garamond",
                "Office stuff, fifty-fifth\tcafé",
                "en-GB",
                all,
                0,
            ),
            ("Noto Serif", "Ελληνικά γράμματα", "el", on, 0),
            ("Noto Naskh Arabic", "مرحبا بالعالم", "ar", on, 1),
            ("Noto Serif Hebrew", "שלום עולם", "he", on, 1),
        ];
        for (family, text, language, ligatures, level) in cases {
            let face = fonts.select(family, 400, false).expect(family);
            let script = text.chars().find_map(Script::of).expect("a script");
            let span = Span {
                len: text.len(),
                style: Style {
                    family: family.to_string(),
                    size: 12 * POINT,
                    weight: 400,
                    italic: false,
                    language: Language::parse(language).expect("a language"),
                    ligatures,
                },
            };
            // The whole, and a part with text either side.
            let start = text.char_indices().nth(2).map_or(0, |(i, _)| i);
            let end = text.char_indices().nth(9).map_or(text.len(), |(i, _)| i);
            for range in [0..text.len(), start..end] {
                let made = afresh(&fonts, text, range.clone(), &span, face, level, script);
                for time in ["first", "second"] {
                    let kept = shape_text(&fonts, text, range.clone(), &span, face, level, script);
                    assert_eq!(kept, made, "{text:?} {range:?}, the {time} time");
                }
            }
        }
        // A plan for each way it was shaped, kept by its face.
        let garamond = fonts
            .select("EB Garamond", 400, false)
            .expect("EB Garamond");
        assert_eq!(fonts.face(garamond).plans.len(), 3);
    }
}
