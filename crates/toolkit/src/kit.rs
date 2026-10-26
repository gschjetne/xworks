//! What every widget draws with: the fonts, the theme, the icons, and
//! labels laid out once and kept; and the shapes the chrome is made of.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use kurbo::Shape;
use scene::{Color, Item, Path, Point, Segment};
use text::units::PIXEL;
use text::{Align, Cursor, Fonts, Language, Ligatures, Paragraph, Span, Style};

use crate::{Icon, Theme};

/// A rectangle in layout units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Its left edge.
    pub x: i64,
    /// Its top.
    pub y: i64,
    /// Its width.
    pub width: i64,
    /// Its height.
    pub height: i64,
}

impl Rect {
    /// The rectangle at (`x`, `y`), `width` by `height`.
    pub const fn new(x: i64, y: i64, width: i64, height: i64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether (`x`, `y`) is inside.
    pub fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Its right edge.
    pub fn right(&self) -> i64 {
        self.x + self.width
    }

    /// Its bottom.
    pub fn bottom(&self) -> i64 {
        self.y + self.height
    }

    /// It made smaller by `by` on every side.
    pub fn inset(&self, by: i64) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            (self.width - 2 * by).max(0),
            (self.height - 2 * by).max(0),
        )
    }

    /// Its centre.
    pub fn centre(&self) -> (i64, i64) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    pub(crate) fn path(&self) -> Path {
        Path::rect(self.x, self.y, self.width, self.height)
    }
}

/// Lucide's icons, by name.
#[derive(Clone, Debug, Default)]
pub struct Icons {
    icons: HashMap<String, Icon>,
}

/// Every icon in `icons/lucide/`, as Lucide's release has them, built into
/// the program: so xWorks doesn't read a file for each as it starts.
const BUNDLED: &[(&str, &str)] = &[
    ("bold", include_str!("../../../icons/lucide/bold.svg")),
    ("check", include_str!("../../../icons/lucide/check.svg")),
    (
        "chevron-down",
        include_str!("../../../icons/lucide/chevron-down.svg"),
    ),
    (
        "chevron-right",
        include_str!("../../../icons/lucide/chevron-right.svg"),
    ),
    ("command", include_str!("../../../icons/lucide/command.svg")),
    ("copy", include_str!("../../../icons/lucide/copy.svg")),
    (
        "file-text",
        include_str!("../../../icons/lucide/file-text.svg"),
    ),
    ("italic", include_str!("../../../icons/lucide/italic.svg")),
    ("minus", include_str!("../../../icons/lucide/minus.svg")),
    (
        "panel-right",
        include_str!("../../../icons/lucide/panel-right.svg"),
    ),
    ("ruler", include_str!("../../../icons/lucide/ruler.svg")),
    (
        "scroll-text",
        include_str!("../../../icons/lucide/scroll-text.svg"),
    ),
    ("search", include_str!("../../../icons/lucide/search.svg")),
    ("square", include_str!("../../../icons/lucide/square.svg")),
    (
        "text-align-center",
        include_str!("../../../icons/lucide/text-align-center.svg"),
    ),
    (
        "text-align-end",
        include_str!("../../../icons/lucide/text-align-end.svg"),
    ),
    (
        "text-align-justify",
        include_str!("../../../icons/lucide/text-align-justify.svg"),
    ),
    (
        "text-align-start",
        include_str!("../../../icons/lucide/text-align-start.svg"),
    ),
    ("x", include_str!("../../../icons/lucide/x.svg")),
];

impl Icons {
    /// Every icon in `icons/lucide/`, built into the program; those that
    /// can't be read are left out, with their names and why.
    pub fn bundled() -> (Icons, Vec<(String, String)>) {
        Icons::from_svgs(
            BUNDLED
                .iter()
                .map(|(name, svg)| (name.to_string(), svg.to_string()))
                .collect(),
        )
    }

    /// The icons `svgs` hold, by name; those that can't be read are left
    /// out, with their names and why.
    pub fn from_svgs(svgs: Vec<(String, String)>) -> (Icons, Vec<(String, String)>) {
        let mut icons = HashMap::new();
        let mut bad = Vec::new();
        for (name, svg) in svgs {
            match Icon::from_svg(&svg) {
                Ok(icon) => {
                    icons.insert(name, icon);
                }
                Err(e) => bad.push((name, e.to_string())),
            }
        }
        (Icons { icons }, bad)
    }

    /// The icon `name`, if there is one.
    pub fn get(&self, name: &str) -> Option<&Icon> {
        self.icons.get(name)
    }

    /// How many icons there are.
    pub fn len(&self) -> usize {
        self.icons.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }
}

/// Labels laid out, kept by their text, family, size and weight, so the
/// chrome isn't shaped again for every frame and every movement of the
/// pointer.
#[derive(Debug, Default)]
pub struct Labels {
    laid: Mutex<HashMap<LabelKey, Arc<text::Layout>>>,
}

/// What a label is kept by: its text, family, size and weight.
type LabelKey = (String, String, i64, u16);

/// How many labels are kept before they're all let go.
const MOST_LABELS: usize = 4096;

impl Labels {
    /// No labels yet.
    pub fn new() -> Labels {
        Labels::default()
    }

    /// `text` in `family`, `size` layout units and `weight`, on one line.
    fn get(
        &self,
        fonts: &Fonts,
        family: &str,
        text: &str,
        size: i64,
        weight: u16,
    ) -> Arc<text::Layout> {
        let key = (text.to_string(), family.to_string(), size, weight);
        if let Some(layout) = self
            .laid
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return layout.clone();
        }
        let spans = [Span {
            len: text.len(),
            style: Style {
                family: family.to_string(),
                size,
                weight,
                italic: false,
                language: Language::und(),
                ligatures: Ligatures::default(),
            },
        }];
        let layout = Arc::new(text::layout(
            fonts,
            &Paragraph {
                text,
                spans: &spans,
                direction: None,
                align: Align::Start,
                width: i64::MAX / 4,
            },
        ));
        // One the fonts lack characters for is laid out again next time,
        // when fonts loaded since, such as the CJK faces, may have them.
        if layout.missing {
            return layout;
        }
        let mut laid = self.laid.lock().unwrap_or_else(PoisonError::into_inner);
        if laid.len() >= MOST_LABELS {
            laid.clear();
        }
        laid.insert(key, layout.clone());
        layout
    }
}

/// A label's type: its size in layout units and its weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Type {
    /// Its size.
    pub size: i64,
    /// Its weight: 400 regular, 500 medium, 600 semibold.
    pub weight: u16,
}

/// What widgets draw with.
#[derive(Clone, Copy, Debug)]
pub struct Kit<'a> {
    /// The fonts.
    pub fonts: &'a Fonts,
    /// The look.
    pub theme: &'a Theme,
    /// The icons.
    pub icons: &'a Icons,
    /// The labels laid out so far.
    pub labels: &'a Labels,
    /// Whether carets show: a blinking caret is gone now and then.
    pub caret: bool,
}

/// `n` pixels, in layout units.
pub(crate) fn px(n: f64) -> i64 {
    (n * PIXEL as f64).round() as i64
}

impl Kit<'_> {
    /// `text` laid out in `ty`.
    pub fn layout(&self, text: &str, ty: Type) -> Arc<text::Layout> {
        self.layout_in(text, None, ty)
    }

    /// `text` laid out in `ty`, in `family`, or the interface's family
    /// for none: a font family's name shown in itself.
    pub fn layout_in(&self, text: &str, family: Option<&str>, ty: Type) -> Arc<text::Layout> {
        self.labels.get(
            self.fonts,
            family.unwrap_or(&self.theme.sizes.family),
            text,
            ty.size,
            ty.weight,
        )
    }

    /// How wide `text` is in `ty`.
    pub fn width(&self, text: &str, ty: Type) -> i64 {
        self.layout(text, ty).lines.first().map_or(0, |l| l.width)
    }

    /// `text`, or as much of it as fits in `width` in `ty` and `family`
    /// with an ellipsis after it, cut between characters: so a label
    /// never runs into what's beside it.
    pub fn fit(&self, text: &str, family: Option<&str>, width: i64, ty: Type) -> String {
        let wide = |t: &str| {
            self.layout_in(t, family, ty)
                .lines
                .first()
                .map_or(0, |l| l.width)
        };
        if wide(text) <= width {
            return text.to_string();
        }
        // Where each character starts: a cut before character k leaves k.
        let cuts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        // The most characters that fit with the ellipsis, by halves; not
        // all of them, which don't fit even without it.
        let (mut lo, mut hi) = (0, cuts.len().saturating_sub(1));
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let shown = format!("{}…", text[..cuts[mid]].trim_end());
            if wide(&shown) <= width {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        match lo {
            0 => "…".to_string(),
            n => format!("{}…", text[..cuts[n]].trim_end()),
        }
    }

    /// The type of `size` at `weight`.
    pub fn ty(&self, size: text::units::Length, weight: u16) -> Type {
        Type {
            size: size.units(),
            weight,
        }
    }

    /// Draws `text` from `x`, its line centred in the row from `top`
    /// `height` tall, in `color`. Returns its width.
    #[allow(
        clippy::too_many_arguments,
        reason = "a label's place, type and colour"
    )]
    pub fn text(
        &self,
        out: &mut Vec<Item>,
        text: &str,
        x: i64,
        top: i64,
        height: i64,
        ty: Type,
        color: Color,
    ) -> i64 {
        self.text_in(out, text, None, x, top, height, ty, color)
    }

    /// Draws `text` as [`Kit::text`] does, in `family`, or the
    /// interface's family for none.
    #[allow(
        clippy::too_many_arguments,
        reason = "a label's place, family, type and colour"
    )]
    pub fn text_in(
        &self,
        out: &mut Vec<Item>,
        text: &str,
        family: Option<&str>,
        x: i64,
        top: i64,
        height: i64,
        ty: Type,
        color: Color,
    ) -> i64 {
        let layout = self.layout_in(text, family, ty);
        let y = top + (height - layout.height) / 2;
        out.extend(layout.paint(self.fonts, Point::new(x, y), None, |_| color));
        layout.lines.first().map_or(0, |l| l.width)
    }

    /// Draws `text` as [`Kit::text`] does, but ending at `right`.
    #[allow(
        clippy::too_many_arguments,
        reason = "a label's place, type and colour"
    )]
    pub fn text_right(
        &self,
        out: &mut Vec<Item>,
        text: &str,
        right: i64,
        top: i64,
        height: i64,
        ty: Type,
        color: Color,
    ) {
        let width = self.width(text, ty);
        self.text(out, text, right - width, top, height, ty, color);
    }

    /// Underlines the character of `text` at byte `at`, as it's drawn from
    /// `x` in the row from `top` `height` tall: an access key.
    #[allow(
        clippy::too_many_arguments,
        reason = "a label's place, type and colour"
    )]
    pub fn underline(
        &self,
        out: &mut Vec<Item>,
        text: &str,
        at: usize,
        x: i64,
        top: i64,
        height: i64,
        ty: Type,
        color: Color,
    ) {
        let layout = self.layout(text, ty);
        let Some(c) = text[at..].chars().next() else {
            return;
        };
        let a = layout.caret(Cursor::downstream(at)).x;
        let b = layout.caret(Cursor::upstream(at + c.len_utf8())).x;
        let y = top + (height - layout.height) / 2;
        let baseline = layout.lines.first().map_or(0, |l| l.baseline);
        out.push(Item::Fill {
            path: Path::rect(x + a.min(b), y + baseline + px(1.5), (b - a).abs(), PIXEL),
            color,
        });
    }

    /// Lucide's icon `name`, `size` across, centred on (`cx`, `cy`).
    pub fn icon(&self, out: &mut Vec<Item>, name: &str, cx: i64, cy: i64, size: i64, color: Color) {
        if let Some(icon) = self.icons.get(name) {
            out.push(icon.draw(cx - size / 2, cy - size / 2, size, color));
        }
    }
}

/// A rectangle filled with `color`.
pub(crate) fn fill(out: &mut Vec<Item>, rect: Rect, color: Color) {
    out.push(Item::Fill {
        path: rect.path(),
        color,
    });
}

/// A rectangle with corners of radius `r`, filled with `color`.
pub(crate) fn rounded(out: &mut Vec<Item>, rect: Rect, r: i64, color: Color) {
    out.push(Item::Fill {
        path: rounded_path(rect, r),
        color,
    });
}

/// A field's box: rounded, with a hairline edge, in the look's field
/// colours.
pub(crate) fn field_box(out: &mut Vec<Item>, colors: &crate::Colors, rect: Rect, r: i64) {
    rounded(out, rect, r, colors.field_line);
    rounded(out, rect.inset(PIXEL), r - PIXEL, colors.field);
}

/// A swatch of `color` filling `rect`, with a hairline edge in the look's
/// field line, so a colour near the chrome's still shows its edge.
pub(crate) fn swatch(out: &mut Vec<Item>, colors: &crate::Colors, rect: Rect, color: Color) {
    let r = px(3.0);
    rounded(out, rect, r, colors.field_line);
    rounded(out, rect.inset(PIXEL), r - PIXEL, color);
}

/// A small triangle with its point at (`x`, `y`), `size` from its point
/// to its base, pointing down if `down`, else up.
pub(crate) fn triangle(out: &mut Vec<Item>, x: i64, y: i64, size: i64, down: bool, color: Color) {
    let base = if down { y - size } else { y + size };
    out.push(Item::Fill {
        path: Path(vec![
            Segment::MoveTo(Point::new(x, y)),
            Segment::LineTo(Point::new(x + size, base)),
            Segment::LineTo(Point::new(x - size, base)),
            Segment::Close,
        ]),
        color,
    });
}

/// A shadow under a panel: a soft one and a close one, in the look's
/// shadow colour.
pub(crate) fn shadow(out: &mut Vec<Item>, colors: &crate::Colors, rect: Rect, r: i64) {
    let soft = Color {
        a: colors.shadow.a / 2,
        ..colors.shadow
    };
    let six = px(6.0);
    rounded(
        out,
        Rect::new(
            rect.x - six,
            rect.y + px(2.0),
            rect.width + 2 * six,
            rect.height + 2 * six,
        ),
        r + 4 * PIXEL,
        soft,
    );
    rounded(
        out,
        Rect::new(
            rect.x - 2 * PIXEL,
            rect.y,
            rect.width + 4 * PIXEL,
            rect.height + 4 * PIXEL,
        ),
        r + PIXEL,
        colors.shadow,
    );
}

/// The path of `rect` with corners of radius `r`, less `hole`: the part
/// of a shadow that shows round what covers it, as only that is drawn.
pub(crate) fn ring_path(rect: Rect, r: i64, hole: Rect) -> Path {
    let f = |v: i64| v as f64 / PIXEL as f64;
    let mut path = kurbo::RoundedRect::new(
        f(rect.x),
        f(rect.y),
        f(rect.right()),
        f(rect.bottom()),
        f(r).max(0.0),
    )
    .to_path(0.01);
    // The hole wound the other way, so the nonzero rule leaves it empty.
    let inner = kurbo::Rect::new(f(hole.x), f(hole.y), f(hole.right()), f(hole.bottom()))
        .to_path(0.01)
        .reverse_subpaths();
    path.extend(inner);
    to_path(&path)
}

/// `path`, in layout units from pixels.
fn to_path(path: &kurbo::BezPath) -> Path {
    let at = |p: kurbo::Point| Point::new(px(p.x), px(p.y));
    Path(
        path.elements()
            .iter()
            .map(|el| match *el {
                kurbo::PathEl::MoveTo(p) => Segment::MoveTo(at(p)),
                kurbo::PathEl::LineTo(p) => Segment::LineTo(at(p)),
                kurbo::PathEl::QuadTo(a, b) => Segment::QuadTo(at(a), at(b)),
                kurbo::PathEl::CurveTo(a, b, c) => Segment::CubicTo(at(a), at(b), at(c)),
                kurbo::PathEl::ClosePath => Segment::Close,
            })
            .collect(),
    )
}

/// The path of `rect` with corners of radius `r`.
pub(crate) fn rounded_path(rect: Rect, r: i64) -> Path {
    let f = |v: i64| v as f64 / PIXEL as f64;
    let shape = kurbo::RoundedRect::new(
        f(rect.x),
        f(rect.y),
        f(rect.right()),
        f(rect.bottom()),
        f(r).max(0.0),
    );
    to_path(&shape.to_path(0.01))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    /// A label the fonts lack characters for isn't kept: every window
    /// shares the labels, and fonts loaded later, such as the CJK faces,
    /// would never show in it.
    #[test]
    fn a_label_missing_characters_is_laid_out_again() {
        let labels = Labels::new();
        let mut latin = Fonts::new();
        for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
            if name == "Inter-Regular.ttf" {
                latin.add(bytes).expect("Inter");
            }
        }
        let label = labels.get(&latin, "Inter", "文件", 13 * PIXEL, 400);
        assert!(label.missing);
        let kept = labels.laid.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(kept.is_empty(), "kept: {:?}", kept.keys());
    }

    #[test]
    fn a_ring_is_empty_where_its_hole_is() {
        let px = |n: i64| n * PIXEL;
        // A shadow a pixel wider than its page each side, from 9 to 102
        // across, and the page from 10 to 101.
        let ring = ring_path(
            Rect::new(px(9), px(10), px(93), px(90)),
            px(2),
            Rect::new(px(10), px(10), px(91), px(90)),
        );
        let mut path = kurbo::BezPath::new();
        let at = |p: Point| kurbo::Point::new(p.x as f64, p.y as f64);
        for segment in &ring.0 {
            match *segment {
                Segment::MoveTo(p) => path.move_to(at(p)),
                Segment::LineTo(p) => path.line_to(at(p)),
                Segment::QuadTo(a, b) => path.quad_to(at(a), at(b)),
                Segment::CubicTo(a, b, c) => path.curve_to(at(a), at(b), at(c)),
                Segment::Close => path.close_path(),
            }
        }
        let point = |x: f64, y: f64| kurbo::Point::new(x * PIXEL as f64, y * PIXEL as f64);
        assert_eq!(path.winding(point(50.0, 50.0)), 0, "the hole");
        assert_ne!(path.winding(point(9.5, 50.0)), 0, "the rim");
        assert_ne!(path.winding(point(101.5, 50.0)), 0, "the rim");
    }
}
