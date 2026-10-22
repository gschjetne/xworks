//! Mockups of the chrome M2's UX design settled (ARCHITECTURE.md section 8,
//! review frhewnk0): pictures of windows, drawn from shapes and text, for
//! the project owner to approve before stage 5 builds them. They aren't
//! the toolkit; stage 5 builds the toolkit to match them.
//!
//! Every window is 1280 by 800 pixels at 96 to the inch.

use std::collections::HashMap;
use std::sync::OnceLock;

use kurbo::Shape;
use scene::{Color, Item, Page, Path, Point, Segment};
use text::units::PIXEL;
use text::{Align, Language, Paragraph, Span, Style};
use toolkit::Icon;

use crate::pages::{document_in, document_pages, fonts};

/// The window's size, in pixels.
const WIDTH: f64 = 1280.0;
const HEIGHT: f64 = 800.0;

/// The inspector's width, the ruler's and the status bar's height, in
/// pixels.
const INSPECTOR: f64 = 300.0;
const RULER: f64 = 26.0;
const STATUS_BAR: f64 = 26.0;

/// An A4 page's width at 96 to the inch, and its side margins (25 mm).
const PAGE_WIDTH: f64 = 793.7;
const MARGIN: f64 = 94.5;

/// The interface's font.
const UI: &str = "Inter";

/// The colours of one look, by role.
struct Look {
    /// Behind the pages: off-white, easier on the eyes than grey.
    desk: Color,
    /// The title bar, the ruler, the inspector and the status bar.
    chrome: Color,
    /// Hairlines between the chrome and the document.
    line: Color,
    ink: Color,
    secondary: Color,
    /// What's under the pointer, or on.
    hover: Color,
    accent: Color,
    /// Fields in the inspector and the palette.
    field: Color,
    field_line: Color,
    /// Shadows on the desk.
    shadow: Color,
}

const LIGHT: Look = Look {
    desk: Color::rgb(246, 245, 241),
    chrome: Color::rgb(239, 238, 233),
    line: Color::rgb(222, 220, 213),
    ink: Color::rgb(31, 30, 28),
    secondary: Color::rgb(110, 108, 102),
    hover: Color::rgb(228, 226, 219),
    accent: Color::rgb(61, 108, 201),
    field: Color::WHITE,
    field_line: Color::rgb(214, 212, 204),
    shadow: Color {
        r: 60,
        g: 50,
        b: 30,
        a: 24,
    },
};

const DARK: Look = Look {
    desk: Color::rgb(30, 30, 28),
    chrome: Color::rgb(38, 38, 36),
    line: Color::rgb(58, 57, 54),
    ink: Color::rgb(236, 235, 230),
    secondary: Color::rgb(163, 161, 154),
    hover: Color::rgb(51, 50, 47),
    accent: Color::rgb(110, 151, 232),
    field: Color::rgb(30, 30, 28),
    field_line: Color::rgb(66, 65, 61),
    shadow: Color {
        r: 0,
        g: 0,
        b: 0,
        a: 90,
    },
};

/// Where the title bar's contextual buttons are drawn.
#[derive(Clone, Copy, PartialEq)]
enum TitleBar {
    /// In a title bar xWorks draws, with the menus and the window's
    /// buttons, as Claude Desktop does.
    Own,
    /// In a bar under the system's title bar, beside the menus.
    System,
}

/// The view a document is shown in.
#[derive(Clone, Copy, PartialEq)]
enum View {
    Pages,
    Draft,
}

/// A label's look: its size in pixels, weight and colour.
#[derive(Clone, Copy)]
struct Type {
    size: f64,
    weight: u16,
    color: Color,
}

/// A window being drawn: its page, in layout units, and its look.
struct Window {
    page: Page,
    look: &'static Look,
}

/// `n` pixels in layout units.
fn px(n: f64) -> i64 {
    (n * PIXEL as f64).round() as i64
}

/// A label's look.
fn ty(size: f64, weight: u16, color: Color) -> Type {
    Type {
        size,
        weight,
        color,
    }
}

impl Window {
    fn new(look: &'static Look) -> Window {
        let mut page = Page::new(px(WIDTH), px(HEIGHT));
        page.fill_background(look.desk);
        Window { page, look }
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color) {
        self.page.items.push(Item::Fill {
            path: Path::rect(px(x), px(y), px(w), px(h)),
            color,
        });
    }

    fn rounded(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64, color: Color) {
        self.page.items.push(Item::Fill {
            path: rounded(x, y, w, h, r),
            color,
        });
    }

    /// A rounded box with a hairline edge, in the look's field colours.
    fn field_box(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let look = self.look;
        self.rounded(x, y, w, h, r, look.field_line);
        self.rounded(x + 1.0, y + 1.0, w - 2.0, h - 2.0, r - 1.0, look.field);
    }

    /// `s` in the interface's font, its line centred in the row from `top`
    /// `height` tall, starting at `x`. Returns its width in pixels.
    fn label(&mut self, s: &str, x: f64, top: f64, height: f64, ty: Type) -> f64 {
        let layout = line(s, px(ty.size), ty.weight);
        let width = layout.lines.first().map_or(0, |l| l.width) as f64 / PIXEL as f64;
        let y = top + (height - layout.height as f64 / PIXEL as f64) / 2.0;
        let color = ty.color;
        self.page
            .items
            .extend(layout.paint(fonts(), Point::new(px(x), px(y)), None, |_| color));
        width
    }

    /// `s` as [`Window::label`] draws it, but ending at `right`.
    fn label_right(&mut self, s: &str, right: f64, top: f64, height: f64, ty: Type) {
        let width = label_width(s, ty);
        self.label(s, right - width, top, height, ty);
    }

    /// Lucide's icon `name`, `size` pixels across, centred on (`cx`, `cy`).
    fn icon(&mut self, name: &str, cx: f64, cy: f64, size: f64, color: Color) {
        let icon = icons()
            .get(name)
            .unwrap_or_else(|| panic!("icons/lucide/{name}.svg"));
        self.page
            .items
            .push(icon.draw(px(cx - size / 2.0), px(cy - size / 2.0), px(size), color));
    }

    /// A small triangle with its point at (`x`, `y`), pointing down if
    /// `down`, else up.
    fn triangle(&mut self, x: f64, y: f64, size: f64, down: bool, color: Color) {
        let base = if down { y - size } else { y + size };
        let p = |x: f64, y: f64| Point::new(px(x), px(y));
        self.page.items.push(Item::Fill {
            path: Path(vec![
                Segment::MoveTo(p(x, y)),
                Segment::LineTo(p(x + size, base)),
                Segment::LineTo(p(x - size, base)),
                Segment::Close,
            ]),
            color,
        });
    }

    /// The menus' titles, from `x`, in the row from `top`.
    fn menus(&mut self, x: f64, top: f64, height: f64) {
        let ink = ty(13.0, 400, self.look.ink);
        let mut x = x;
        for title in ["File", "Edit", "View", "Insert", "Format", "Help"] {
            x += self.label(title, x, top, height, ink) + 18.0;
        }
    }

    /// Two choices as one control, `w` by `h` at (`x`, `y`), the first on
    /// if `first`.
    #[allow(clippy::too_many_arguments, reason = "a control's place and its words")]
    fn segmented(&mut self, x: f64, y: f64, w: f64, h: f64, words: [&str; 2], first: bool) {
        let look = self.look;
        self.rounded(x, y, w, h, 6.0, look.hover);
        let on = if first { x } else { x + w / 2.0 };
        self.field_box(on + 2.0, y + 2.0, w / 2.0 - 4.0, h - 4.0, 5.0);
        for (i, word) in words.iter().enumerate() {
            let color = if (i == 0) == first {
                look.ink
            } else {
                look.secondary
            };
            let words_ty = ty(12.0, 500, color);
            let ww = label_width(word, words_ty);
            let cx = x + w / 4.0 + i as f64 * w / 2.0;
            self.label(word, cx - ww / 2.0, y, h, words_ty);
        }
    }

    /// The contextual buttons, ending at `right`, centred in the row from
    /// `top`: the view, and the inspector's button, on if `inspector`.
    fn contextual(&mut self, right: f64, top: f64, height: f64, view: View, inspector: bool) {
        let look = self.look;
        let mid = top + height / 2.0;
        let button = 28.0;
        let bx = right - button;
        if inspector {
            self.rounded(bx, mid - button / 2.0, button, button, 6.0, look.hover);
        }
        self.icon("panel-right", bx + button / 2.0, mid, 16.0, look.ink);
        let (w, h) = (128.0, 26.0);
        self.segmented(
            bx - 10.0 - w,
            mid - h / 2.0,
            w,
            h,
            ["Draft", "Pages"],
            view == View::Draft,
        );
    }

    /// The chrome above the document, as `bar` places it. Returns where the
    /// document's area starts.
    fn top(&mut self, bar: TitleBar, title: &str, view: View, inspector: bool) -> f64 {
        let look = self.look;
        match bar {
            TitleBar::Own => {
                let h = 40.0;
                self.rect(0.0, 0.0, WIDTH, h, look.chrome);
                self.menus(14.0, 0.0, h);
                let title_ty = ty(13.0, 500, look.secondary);
                let tw = label_width(title, title_ty);
                self.label(title, (WIDTH - tw) / 2.0, 0.0, h, title_ty);
                // The window's own buttons: minimise, maximise, close.
                for (i, name) in ["minus", "square", "x"].iter().enumerate() {
                    let cx = WIDTH - 46.0 * (3 - i) as f64 + 23.0;
                    let size = if *name == "square" { 13.0 } else { 16.0 };
                    self.icon(name, cx, h / 2.0, size, look.ink);
                }
                self.contextual(WIDTH - 3.0 * 46.0 - 12.0, 0.0, h, view, inspector);
                self.rect(0.0, h - 1.0, WIDTH, 1.0, look.line);
                h
            }
            TitleBar::System => {
                // The system's title bar, as Windows 11 draws it.
                let sys = 32.0;
                let (bg, fg) = if look.ink.r > 128 {
                    (Color::rgb(32, 32, 32), Color::rgb(255, 255, 255))
                } else {
                    (Color::rgb(243, 243, 243), Color::rgb(28, 28, 28))
                };
                self.rect(0.0, 0.0, WIDTH, sys, bg);
                let title = format!("{title} — xWorks");
                self.label(&title, 14.0, 0.0, sys, ty(12.0, 400, fg));
                for (i, name) in ["minus", "square", "x"].iter().enumerate() {
                    let cx = WIDTH - 46.0 * (3 - i) as f64 + 23.0;
                    let size = if *name == "square" { 11.0 } else { 14.0 };
                    self.icon(name, cx, sys / 2.0, size, fg);
                }
                // xWorks' own bar: the menus, and the contextual buttons.
                let h = 34.0;
                self.rect(0.0, sys, WIDTH, h, look.chrome);
                self.menus(14.0, sys, h);
                self.contextual(WIDTH - 12.0, sys, h, view, inspector);
                self.rect(0.0, sys + h - 1.0, WIDTH, 1.0, look.line);
                sys + h
            }
        }
    }

    /// The ruler, from `top`, across `width`, for a text column from `left`
    /// `column` wide.
    fn ruler(&mut self, top: f64, width: f64, left: f64, column: f64) {
        let look = self.look;
        let h = RULER;
        self.rect(0.0, top, width, h, look.chrome);
        self.rect(left, top + 3.0, column, h - 6.0, look.field);
        // Ticks along the foot every half centimetre, and each whole one
        // numbered above its tick.
        let cm = 96.0 / 2.54;
        let number = ty(9.0, 400, look.secondary);
        let mut n = 1;
        while n as f64 * cm / 2.0 < column - 1.0 {
            let x = (left + n as f64 * cm / 2.0).round();
            if n % 2 == 0 {
                let s = (n / 2).to_string();
                let w = label_width(&s, number);
                self.label(&s, x - w / 2.0 + 0.5, top + 3.0, 13.0, number);
                self.rect(x, top + h - 8.0, 1.0, 4.0, look.secondary);
            } else {
                self.rect(x, top + h - 6.0, 1.0, 2.0, look.secondary);
            }
            n += 1;
        }
        // Indents: the first line's above, the left and right below.
        self.triangle(left, top + 3.0 + 4.5, 4.5, true, look.ink);
        self.triangle(left, top + h - 3.0 - 4.5, 4.5, false, look.ink);
        self.triangle(left + column, top + h - 3.0 - 4.5, 4.5, false, look.ink);
        // Tab stops: left, centred, right and on the decimal point.
        let foot = top + h - 4.5;
        let ink = look.accent;
        for (c, kind, _) in TABS {
            let x = (left + c * cm).round();
            self.rect(x - 0.75, foot - 8.0, 1.5, 8.0, ink);
            match kind {
                Tab::Left => self.rect(x - 0.75, foot - 1.5, 6.0, 1.5, ink),
                Tab::Right => self.rect(x - 5.25, foot - 1.5, 6.0, 1.5, ink),
                Tab::Centred | Tab::Decimal => self.rect(x - 4.0, foot - 1.5, 8.0, 1.5, ink),
            }
            if kind == Tab::Decimal {
                self.rect(x + 2.0, foot - 5.0, 2.0, 2.0, ink);
            }
        }
        self.rect(0.0, top + h - 1.0, width, 1.0, look.line);
    }

    /// The status bar along the foot.
    fn status(&mut self, left: &str) {
        let look = self.look;
        let top = HEIGHT - STATUS_BAR;
        self.rect(0.0, top, WIDTH, STATUS_BAR, look.chrome);
        self.rect(0.0, top, WIDTH, 1.0, look.line);
        let quiet = ty(12.0, 400, look.secondary);
        self.label(left, 14.0, top, STATUS_BAR, quiet);
        self.label_right("Saved", WIDTH - 14.0, top, STATUS_BAR, quiet);
    }

    /// The first page of the sample document, with its top left at (`x`,
    /// `y`) in pixels, on a soft shadow.
    fn page_at(&mut self, x: f64, y: f64) {
        let shadow = self.look.shadow;
        self.rounded(x - 1.0, y, PAGE_WIDTH + 2.0, 1200.0, 2.0, shadow);
        self.rounded(x, y + 1.0, PAGE_WIDTH, 1200.0, 2.0, shadow);
        let pages = document_pages();
        self.page.place(&pages.pages[0], Point::new(px(x), px(y)));
    }

    /// The inspector, from `top` down to the status bar.
    fn inspector(&mut self, top: f64) {
        let look = self.look;
        let x0 = WIDTH - INSPECTOR;
        let bottom = HEIGHT - STATUS_BAR;
        self.rect(x0, top, INSPECTOR, bottom - top, look.chrome);
        self.rect(x0, top, 1.0, bottom - top, look.line);
        let (pad, label_w, row) = (16.0, 74.0, 34.0);
        let fx = x0 + pad + label_w;
        let fw = INSPECTOR - 2.0 * pad - label_w;
        let half = (fw - 8.0) / 2.0;
        let heading = ty(11.0, 600, look.secondary);
        let name = ty(12.0, 400, look.secondary);
        let value = ty(12.5, 400, look.ink);
        let mut y = top + 12.0;
        self.segmented(
            x0 + pad,
            y,
            INSPECTOR - 2.0 * pad,
            28.0,
            ["Text", "Page"],
            true,
        );
        y += 44.0;

        let field = |win: &mut Window, y: f64, label: &str, text: &str| {
            win.label(label, x0 + pad, y, 28.0, name);
            win.field_box(fx, y, fw, 28.0, 5.0);
            win.label(text, fx + 10.0, y, 28.0, value);
            win.icon(
                "chevron-down",
                fx + fw - 14.0,
                y + 14.0,
                14.0,
                look.secondary,
            );
        };

        self.label("PARAGRAPH", x0 + pad, y, 20.0, heading);
        y += 26.0;
        field(self, y, "Style", "Body");
        y += row;
        self.label("Align", x0 + pad, y, 28.0, name);
        self.rounded(fx, y, fw, 28.0, 6.0, look.hover);
        let seg = fw / 4.0;
        self.field_box(fx + 2.0, y + 2.0, seg - 4.0, 24.0, 5.0);
        let aligns = [
            "text-align-start",
            "text-align-center",
            "text-align-end",
            "text-align-justify",
        ];
        for (i, icon) in aligns.iter().enumerate() {
            let color = if i == 0 { look.ink } else { look.secondary };
            self.icon(icon, fx + seg * (i as f64 + 0.5), y + 14.0, 15.0, color);
        }
        y += row;
        self.label("Spacing", x0 + pad, y, 28.0, name);
        for (i, text) in ["↑  0 pt", "↓  6 pt"].iter().enumerate() {
            let bx = fx + i as f64 * (half + 8.0);
            self.field_box(bx, y, half, 28.0, 5.0);
            self.label(text, bx + 10.0, y, 28.0, value);
        }
        y += row;
        field(self, y, "Line", "1.2 × the font");
        y += row + 14.0;

        self.label("CHARACTER", x0 + pad, y, 20.0, heading);
        y += 26.0;
        field(self, y, "Font", "Noto Serif");
        y += row;
        self.label("Size", x0 + pad, y, 28.0, name);
        self.field_box(fx, y, half, 28.0, 5.0);
        self.label("11 pt", fx + 10.0, y, 28.0, value);
        let bx = fx + half + 8.0;
        self.rounded(bx, y, half, 28.0, 6.0, look.hover);
        self.icon("bold", bx + half / 4.0, y + 14.0, 15.0, look.ink);
        self.icon("italic", bx + 3.0 * half / 4.0, y + 14.0, 15.0, look.ink);
        y += row;
        field(self, y, "Language", "English (UK)");
        y += row + 14.0;

        self.label("TAB STOPS", x0 + pad, y, 20.0, heading);
        y += 26.0;
        for (at, _, kind) in TABS {
            self.label(&format!("{at} cm"), x0 + pad, y, 24.0, value);
            self.label(kind, fx, y, 24.0, name);
            y += 26.0;
        }
        let link = ty(12.5, 500, look.accent);
        self.label("Add a tab stop", x0 + pad, y + 2.0, 24.0, link);
    }

    /// The command palette, open with `typed` and the commands it finds:
    /// each one's name, its kind and its shortcut.
    fn palette(&mut self, top: f64, typed: &str, found: &[(&str, &str, &str)]) {
        let look = self.look;
        let (w, row) = (560.0, 34.0);
        let x = (WIDTH - w) / 2.0;
        let h = 52.0 + found.len() as f64 * row + 10.0;
        // A soft shadow, then the panel.
        let soft = Color {
            a: look.shadow.a / 2,
            ..look.shadow
        };
        self.rounded(x - 6.0, top + 2.0, w + 12.0, h + 12.0, 16.0, soft);
        self.rounded(x - 2.0, top, w + 4.0, h + 4.0, 13.0, look.shadow);
        self.rounded(x, top, w, h, 12.0, look.field_line);
        self.rounded(x + 1.0, top + 1.0, w - 2.0, h - 2.0, 11.0, look.field);
        self.icon("search", x + 24.0, top + 26.0, 17.0, look.secondary);
        let tw = self.label(typed, x + 44.0, top, 52.0, ty(15.0, 400, look.ink));
        self.rect(x + 45.0 + tw, top + 16.0, 1.5, 20.0, look.ink);
        self.rect(x + 1.0, top + 52.0, w - 2.0, 1.0, look.line);
        let strong = ty(13.5, 500, look.ink);
        let quiet = ty(12.5, 400, look.secondary);
        let mut y = top + 58.0;
        for (i, (name, kind, keys)) in found.iter().enumerate() {
            if i == 0 {
                self.rounded(x + 6.0, y, w - 12.0, row, 7.0, look.hover);
            }
            let nw = self.label(name, x + 18.0, y, row, strong);
            self.label(kind, x + 18.0 + nw + 10.0, y, row, quiet);
            self.label_right(keys, x + w - 18.0, y, row, quiet);
            y += row;
        }
    }
}

/// Tab stops' kinds.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Left,
    Centred,
    Right,
    Decimal,
}

/// The tab stops the ruler and the inspector show: where, in
/// centimetres from the column's left, their kind, and its name.
const TABS: [(f64, Tab, &str); 4] = [
    (3.5, Tab::Left, "Left"),
    (7.5, Tab::Centred, "Centred"),
    (11.5, Tab::Right, "Right"),
    (13.75, Tab::Decimal, "Decimal"),
];

/// One line of `s` in the interface's font, `size` layout units, `weight`.
fn line(s: &str, size: i64, weight: u16) -> text::Layout {
    let spans = [Span {
        len: s.len(),
        style: Style {
            family: UI.to_string(),
            size,
            weight,
            italic: false,
            language: Language::und(),
        },
    }];
    text::layout(
        fonts(),
        &Paragraph {
            text: s,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: i64::MAX / 4,
        },
    )
}

/// How wide `s` is in pixels, as `ty` sets it.
fn label_width(s: &str, ty: Type) -> f64 {
    let layout = line(s, px(ty.size), ty.weight);
    layout.lines.first().map_or(0, |l| l.width) as f64 / PIXEL as f64
}

/// A rectangle with rounded corners, in pixels.
fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64) -> Path {
    let shape = kurbo::RoundedRect::new(x, y, x + w, y + h, r.max(0.0));
    let at = |p: kurbo::Point| Point::new(px(p.x), px(p.y));
    Path(
        shape
            .path_elements(0.01)
            .map(|el| match el {
                kurbo::PathEl::MoveTo(p) => Segment::MoveTo(at(p)),
                kurbo::PathEl::LineTo(p) => Segment::LineTo(at(p)),
                kurbo::PathEl::QuadTo(a, b) => Segment::QuadTo(at(a), at(b)),
                kurbo::PathEl::CurveTo(a, b, c) => Segment::CubicTo(at(a), at(b), at(c)),
                kurbo::PathEl::ClosePath => Segment::Close,
            })
            .collect(),
    )
}

/// Lucide's icons, by name.
fn icons() -> &'static HashMap<String, Icon> {
    static ICONS: OnceLock<HashMap<String, Icon>> = OnceLock::new();
    ICONS.get_or_init(|| {
        testkit::icons()
            .expect("icons/lucide")
            .into_iter()
            .map(|(name, svg)| {
                let icon = Icon::from_svg(&svg).unwrap_or_else(|e| panic!("{name}: {e}"));
                (name, icon)
            })
            .collect()
    })
}

const STATUS: &str = "Page 1 of 3   ·   Body   ·   11 pt   ·   968 words";

/// A document on pages, with the ruler and the inspector open, in a title
/// bar of `bar`'s kind and `look`.
fn pages_window(bar: TitleBar, look: &'static Look) -> Page {
    let mut win = Window::new(look);
    let top = win.top(bar, "A walking skeleton", View::Pages, true);
    let area = WIDTH - INSPECTOR;
    let page_x = ((area - PAGE_WIDTH) / 2.0).round();
    win.page_at(page_x, top + RULER + 22.0);
    win.ruler(top, area, page_x + MARGIN, PAGE_WIDTH - 2.0 * MARGIN);
    win.inspector(top);
    win.status(STATUS);
    win.page
}

/// Pages, in xWorks' own title bar.
pub(crate) fn pages_own_title() -> Page {
    pages_window(TitleBar::Own, &LIGHT)
}

/// Pages, in xWorks' own title bar, dark: the page stays white.
pub(crate) fn pages_dark() -> Page {
    pages_window(TitleBar::Own, &DARK)
}

/// Pages, under the system's title bar.
pub(crate) fn pages_system_title() -> Page {
    pages_window(TitleBar::System, &LIGHT)
}

/// A Markdown file in the draft view, dark, the inspector shut.
pub(crate) fn draft_dark() -> Page {
    let mut win = Window::new(&DARK);
    let top = win.top(TitleBar::Own, "README.md", View::Draft, false);
    let column = 640.0;
    let x = ((WIDTH - column) / 2.0).round();
    win.ruler(top, WIDTH, x, column);
    // The sample's text in a column, in the look's ink, on the desk.
    let style = canvas::PageStyle {
        width: px(column),
        height: 100 * text::units::INCH,
        top: 0,
        bottom: 0,
        left: 0,
        right: 0,
    };
    let mut drawn = document_in(&style).pages.swap_remove(0);
    // Its white background goes, and its black text takes the ink.
    drawn.items.remove(0);
    for item in &mut drawn.items {
        if let Item::Glyphs(g) = item {
            g.color = DARK.ink;
        }
    }
    win.page
        .place(&drawn, Point::new(px(x), px(top + RULER + 36.0)));
    win.status("Markdown   ·   Body   ·   968 words");
    win.page
}

/// The command palette, open over a document on pages.
pub(crate) fn palette() -> Page {
    let mut win = Window::new(&LIGHT);
    let top = win.top(TitleBar::Own, "A walking skeleton", View::Pages, false);
    let page_x = ((WIDTH - PAGE_WIDTH) / 2.0).round();
    win.page_at(page_x, top + RULER + 22.0);
    win.ruler(top, WIDTH, page_x + MARGIN, PAGE_WIDTH - 2.0 * MARGIN);
    win.status(STATUS);
    win.palette(
        top + 36.0,
        "head",
        &[
            ("Heading 1", "Paragraph style", "Ctrl+1"),
            ("Heading 2", "Paragraph style", "Ctrl+2"),
            ("Heading 3", "Paragraph style", "Ctrl+3"),
            ("Go to Heading…", "View", ""),
            ("Headers and Footers", "Insert", ""),
        ],
    );
    win.page
}
