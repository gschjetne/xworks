//! The generic shell's toolkit: draws the chrome model with the documents'
//! renderer and turns pointing into commands (docs/ARCHITECTURE.md, section
//! 8). M1 has its first form:
//!
//! - [`MenuBar`]: the menu bar and the menu open from it, laid out, drawn,
//!   and hit-tested.
//! - [`status_bar`]: a line of text along the foot of a window.
//! - [`List`]: a panel of choices, such as documents to open.
//!
//! Everything is in layout units, with the window's top left at the origin,
//! and drawn with the bundled fonts in [`Theme`]'s sizes.
//!
//! Invariants:
//!
//! - Drawing and hit-testing use the same layout: what's drawn where a
//!   click lands is what the click hits.
//! - Disabled commands and separators are never hit.

use std::ops::Range;

use chrome::MenuItem;
use scene::{Color, Item, Path, Point};
use text::units::PIXEL;
use text::{Align, Fonts, Language, Paragraph, Span, Style};

/// The chrome's look: its font, sizes and colours.
#[derive(Clone, Debug)]
pub struct Theme {
    /// The font family.
    pub family: String,
    /// The font size.
    pub size: i64,
    /// The menu bar's height.
    pub bar_height: i64,
    /// The height of a menu item.
    pub item_height: i64,
    /// Space at a label's sides.
    pub padding: i64,
    /// The chrome's background.
    pub background: Color,
    /// Its text.
    pub ink: Color,
    /// Text that can't be chosen.
    pub disabled: Color,
    /// What's under the pointer, or open.
    pub highlight: Color,
    /// Text on the highlight.
    pub highlight_ink: Color,
    /// Lines between things.
    pub line: Color,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            family: "Noto Sans".to_string(),
            size: 9 * text::units::POINT + text::units::POINT / 2,
            bar_height: 26 * PIXEL,
            item_height: 24 * PIXEL,
            padding: 10 * PIXEL,
            background: Color::rgb(240, 240, 238),
            ink: Color::rgb(20, 20, 20),
            disabled: Color::rgb(150, 150, 150),
            highlight: Color::rgb(40, 90, 200),
            highlight_ink: Color::WHITE,
            line: Color::rgb(200, 200, 196),
        }
    }
}

/// A rectangle in layout units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    /// Whether (`x`, `y`) is inside.
    pub fn contains(&self, x: i64, y: i64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    fn path(&self) -> Path {
        Path::rect(self.x, self.y, self.width, self.height)
    }
}

/// What a point hits in the menu bar or its open menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuHit {
    /// A menu's title, by index.
    Title(usize),
    /// A command in the open menu, by ID.
    Command(String),
    /// Somewhere else in the open menu: a separator, a disabled command, the
    /// margin.
    Menu,
}

/// The menu bar laid out: where its titles are, and the open menu's items.
#[derive(Clone, Debug)]
pub struct MenuBar {
    bar: chrome::MenuBar,
    width: i64,
    titles: Vec<Range<i64>>,
    open: Option<OpenMenu>,
}

/// The open menu laid out: which it is, its rectangle, and each item's row
/// with the item's index.
#[derive(Clone, Debug)]
struct OpenMenu {
    index: usize,
    rect: Rect,
    rows: Vec<(Rect, usize)>,
}

fn label(fonts: &Fonts, theme: &Theme, text: &str, bold: bool) -> text::Layout {
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: theme.family.clone(),
            size: theme.size,
            weight: if bold { 700 } else { 400 },
            italic: false,
            language: Language::und(),
        },
    }];
    text::layout(
        fonts,
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: i64::MAX / 4,
        },
    )
}

fn width_of(layout: &text::Layout) -> i64 {
    layout.lines.first().map_or(0, |l| l.width)
}

/// Draws `text` from `row`'s left edge, its line centred in the row's
/// height.
fn draw_label(
    fonts: &Fonts,
    theme: &Theme,
    out: &mut Vec<Item>,
    text: &str,
    row: Rect,
    color: Color,
) {
    let layout = label(fonts, theme, text, false);
    let y = row.y + (row.height - layout.height) / 2;
    out.extend(layout.paint(fonts, Point::new(row.x, y), None, |_| color));
}

/// A row for a label: from `x`, between `y` and `y + height`.
fn row(x: i64, y: i64, height: i64) -> Rect {
    Rect {
        x,
        y,
        width: 0,
        height,
    }
}

impl MenuBar {
    /// `bar` laid out across `width`, with menu `open` open, if any.
    pub fn new(
        fonts: &Fonts,
        theme: &Theme,
        bar: chrome::MenuBar,
        width: i64,
        open: Option<usize>,
    ) -> MenuBar {
        let mut titles = Vec::new();
        let mut x = theme.padding / 2;
        for menu in &bar.menus {
            let w = width_of(&label(fonts, theme, &menu.title, false)) + 2 * theme.padding;
            titles.push(x..x + w);
            x += w;
        }
        let open = open.filter(|&i| i < bar.menus.len()).map(|index| {
            let i = index;
            let menu = &bar.menus[i];
            let mut widest = 0;
            for item in &menu.items {
                if let MenuItem::Command {
                    title, shortcut, ..
                } = item
                {
                    let w = width_of(&label(fonts, theme, title, false))
                        + shortcut.as_ref().map_or(0, |s| {
                            width_of(&label(fonts, theme, s, false)) + 3 * theme.padding
                        });
                    widest = widest.max(w);
                }
            }
            let left = titles[i].start;
            let width = widest + 4 * theme.padding;
            let mut y = theme.bar_height + theme.padding / 3;
            let mut items = Vec::new();
            for (k, item) in menu.items.iter().enumerate() {
                let h = match item {
                    MenuItem::Separator => theme.item_height / 3,
                    MenuItem::Command { .. } => theme.item_height,
                };
                items.push((
                    Rect {
                        x: left,
                        y,
                        width,
                        height: h,
                    },
                    k,
                ));
                y += h;
            }
            let rect = Rect {
                x: left,
                y: theme.bar_height,
                width,
                height: y - theme.bar_height + theme.padding / 3,
            };
            OpenMenu {
                index,
                rect,
                rows: items,
            }
        });
        MenuBar {
            bar,
            width,
            titles,
            open,
        }
    }

    /// The bar's height.
    pub fn height(theme: &Theme) -> i64 {
        theme.bar_height
    }

    /// Which menu is open.
    pub fn open(&self) -> Option<usize> {
        self.open.as_ref().map(|o| o.index)
    }

    /// What (`x`, `y`) hits, if anything of the bar or its open menu.
    pub fn hit(&self, theme: &Theme, x: i64, y: i64) -> Option<MenuHit> {
        if let Some(open) = &self.open
            && open.rect.contains(x, y)
        {
            let menu = &self.bar.menus[open.index];
            for (r, k) in &open.rows {
                if r.contains(x, y)
                    && let MenuItem::Command {
                        id, enabled: true, ..
                    } = &menu.items[*k]
                {
                    return Some(MenuHit::Command(id.clone()));
                }
            }
            return Some(MenuHit::Menu);
        }
        if y >= 0 && y < theme.bar_height {
            return self
                .titles
                .iter()
                .position(|r| r.contains(&x))
                .map(MenuHit::Title);
        }
        None
    }

    /// The bar drawn, with its open menu, highlighting what `hover` hits.
    pub fn draw(&self, fonts: &Fonts, theme: &Theme, hover: Option<&MenuHit>) -> Vec<Item> {
        let mut out = vec![
            Item::Fill {
                path: Path::rect(0, 0, self.width, theme.bar_height),
                color: theme.background,
            },
            Item::Fill {
                path: Path::rect(0, theme.bar_height - PIXEL, self.width, PIXEL),
                color: theme.line,
            },
        ];
        for (i, (menu, range)) in self.bar.menus.iter().zip(&self.titles).enumerate() {
            let lit = self.open() == Some(i) || hover == Some(&MenuHit::Title(i));
            if lit {
                out.push(Item::Fill {
                    path: Path::rect(
                        range.start,
                        0,
                        range.end - range.start,
                        theme.bar_height - PIXEL,
                    ),
                    color: theme.highlight,
                });
            }
            let ink = if lit { theme.highlight_ink } else { theme.ink };
            draw_label(
                fonts,
                theme,
                &mut out,
                &menu.title,
                row(range.start + theme.padding, 0, theme.bar_height),
                ink,
            );
        }
        if let Some(OpenMenu { index, rect, rows }) = &self.open {
            let menu = &self.bar.menus[*index];
            // A shadow, the menu, and its edge.
            out.push(Item::Fill {
                path: Path::rect(
                    rect.x + 2 * PIXEL,
                    rect.y + 2 * PIXEL,
                    rect.width,
                    rect.height,
                ),
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 40,
                },
            });
            out.push(Item::Fill {
                path: rect.path(),
                color: Color::WHITE,
            });
            out.push(Item::Stroke {
                path: rect.path(),
                width: PIXEL,
                color: theme.line,
            });
            for (r, k) in rows {
                match &menu.items[*k] {
                    MenuItem::Separator => out.push(Item::Fill {
                        path: Path::rect(
                            r.x + theme.padding,
                            r.y + r.height / 2,
                            r.width - 2 * theme.padding,
                            PIXEL,
                        ),
                        color: theme.line,
                    }),
                    MenuItem::Command {
                        id,
                        title,
                        shortcut,
                        enabled,
                        checked,
                    } => {
                        let lit = *enabled && hover == Some(&MenuHit::Command(id.clone()));
                        if lit {
                            out.push(Item::Fill {
                                path: r.path(),
                                color: theme.highlight,
                            });
                        }
                        let ink = match (lit, enabled) {
                            (true, _) => theme.highlight_ink,
                            (false, true) => theme.ink,
                            (false, false) => theme.disabled,
                        };
                        if *checked {
                            draw_label(
                                fonts,
                                theme,
                                &mut out,
                                "✓",
                                row(r.x + theme.padding / 2, r.y, r.height),
                                ink,
                            );
                        }
                        draw_label(
                            fonts,
                            theme,
                            &mut out,
                            title,
                            row(r.x + 2 * theme.padding, r.y, r.height),
                            ink,
                        );
                        if let Some(shortcut) = shortcut {
                            let w = width_of(&label(fonts, theme, shortcut, false));
                            draw_label(
                                fonts,
                                theme,
                                &mut out,
                                shortcut,
                                row(r.x + r.width - theme.padding - w, r.y, r.height),
                                if *enabled { theme.disabled } else { ink },
                            );
                        }
                    }
                }
            }
        }
        out
    }
}

/// A status bar `width` wide with its bottom at `bottom`, showing `left`
/// and `right` at its ends. Returns what's drawn and its height.
pub fn status_bar(
    fonts: &Fonts,
    theme: &Theme,
    left: &str,
    right: &str,
    width: i64,
    bottom: i64,
) -> (Vec<Item>, i64) {
    let height = theme.bar_height;
    let top = bottom - height;
    let mut out = vec![
        Item::Fill {
            path: Path::rect(0, top, width, height),
            color: theme.background,
        },
        Item::Fill {
            path: Path::rect(0, top, width, PIXEL),
            color: theme.line,
        },
    ];
    draw_label(
        fonts,
        theme,
        &mut out,
        left,
        row(theme.padding, top, height),
        theme.ink,
    );
    let w = width_of(&label(fonts, theme, right, false));
    draw_label(
        fonts,
        theme,
        &mut out,
        right,
        row(width - theme.padding - w, top, height),
        theme.ink,
    );
    (out, height)
}

/// A panel of choices, one a line, with one selected: for picking a
/// document to open.
#[derive(Clone, Debug)]
pub struct List {
    rect: Rect,
    title: String,
    choices: Vec<String>,
}

impl List {
    /// A panel with `title` and `choices`, centred in a window `width` by
    /// `height`.
    pub fn new(theme: &Theme, title: &str, choices: Vec<String>, width: i64, height: i64) -> List {
        let w = (width * 2 / 3).clamp(200 * PIXEL, 640 * PIXEL).min(width);
        let shown = choices.len().clamp(1, 14) as i64;
        let h = (theme.item_height * (shown + 2)).min(height);
        List {
            rect: Rect {
                x: (width - w) / 2,
                y: (height - h) / 3,
                width: w,
                height: h,
            },
            title: title.to_string(),
            choices,
        }
    }

    /// Which choice (`x`, `y`) hits; `Some(None)` inside the panel but on no
    /// choice, `None` outside it.
    pub fn hit(&self, theme: &Theme, x: i64, y: i64) -> Option<Option<usize>> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let row = (y - self.rect.y - theme.item_height) / theme.item_height;
        Some(
            usize::try_from(row)
                .ok()
                .filter(|&r| r < self.choices.len().min(14)),
        )
    }

    /// The panel drawn, with choice `selected` highlighted.
    pub fn draw(&self, fonts: &Fonts, theme: &Theme, selected: Option<usize>) -> Vec<Item> {
        let r = self.rect;
        let mut out = vec![
            Item::Fill {
                path: Path::rect(r.x + 3 * PIXEL, r.y + 3 * PIXEL, r.width, r.height),
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 50,
                },
            },
            Item::Fill {
                path: r.path(),
                color: Color::WHITE,
            },
            Item::Stroke {
                path: r.path(),
                width: PIXEL,
                color: theme.line,
            },
            Item::Fill {
                path: Path::rect(r.x, r.y, r.width, theme.item_height),
                color: theme.background,
            },
        ];
        draw_label(
            fonts,
            theme,
            &mut out,
            &self.title,
            row(r.x + theme.padding, r.y, theme.item_height),
            theme.ink,
        );
        if self.choices.is_empty() {
            draw_label(
                fonts,
                theme,
                &mut out,
                "Nothing here yet.",
                row(
                    r.x + theme.padding,
                    r.y + theme.item_height,
                    theme.item_height,
                ),
                theme.disabled,
            );
        }
        for (i, choice) in self.choices.iter().take(14).enumerate() {
            let y = r.y + theme.item_height * (i as i64 + 1);
            let lit = selected == Some(i);
            if lit {
                out.push(Item::Fill {
                    path: Path::rect(r.x, y, r.width, theme.item_height),
                    color: theme.highlight,
                });
            }
            let ink = if lit { theme.highlight_ink } else { theme.ink };
            draw_label(
                fonts,
                theme,
                &mut out,
                choice,
                row(r.x + theme.padding, y, theme.item_height),
                ink,
            );
        }
        out
    }
}
