//! Drawing the editor's area: the pages in view, or the draft's column on
//! the desk, then the selection, the caret and the composition. The window
//! draws the chrome around it.

use part::Look;
use scene::{Color, Item, Page, Path, Point};
use text::units::PIXEL;
use toolkit::Rect;

use crate::Editor;

pub(crate) fn draw(editor: &Editor, look: &Look) -> Page {
    let res = editor.resources();
    let parts = editor.view_parts();
    let fonts = res.fonts();
    let style = editor.page_style();
    let mut out = Page::new(parts.width, parts.height);
    out.fill_background(look.desk);

    // The pages in view, and their text; or the draft's, on the desk.
    let (view_top, view_bottom) = (parts.top, parts.top + parts.view_height);
    for n in 0..parts.pages {
        let (x, y) = editor.origin_of_page(n);
        let height = if parts.draft {
            editor.draft_bottom()
        } else {
            style.height
        };
        if y + height < view_top || y > view_bottom {
            continue;
        }
        if !parts.draft {
            // A soft shadow, as the mockups have it, and the page.
            let r = 2 * PIXEL;
            for (dx, dy, dw) in [(-PIXEL, 0, 2 * PIXEL), (0, PIXEL, 0)] {
                out.items.push(Item::Fill {
                    path: toolkit::rounded(
                        Rect::new(x + dx, y + dy, style.width + dw, style.height),
                        r,
                    ),
                    color: look.shadow,
                });
            }
            out.items.push(Item::Fill {
                path: Path::rect(x, y, style.width, style.height),
                color: look.page,
            });
        }
        let painted = canvas::paint_page(
            &res.sheet.blocks,
            canvas::Poured {
                paragraphs: parts.paragraphs,
                units: parts.units,
                blocks: parts.blocks,
                pictures: parts.pictures,
            },
            &parts.pieces[n],
            Point::new(x + style.left, y + style.top),
            fonts,
            None,
        );
        if parts.draft {
            out.items
                .extend(painted.into_iter().map(|item| on_desk(item, res, look)));
        } else {
            out.items.extend(painted);
        }
    }

    // The selection, a rectangle for each stretch together on screen.
    if let Some((first, from, last, to)) = parts.selection {
        for k in first..=last {
            let layout = &parts.paragraphs[k].layout;
            let start = if k == first { from } else { 0 };
            let end = if k == last { to } else { usize::MAX / 2 };
            highlight(
                editor,
                &mut out,
                k,
                layout,
                start..end,
                look.selection,
                false,
            );
        }
    }
    let ink = if parts.draft { look.ink } else { look.page_ink };
    // The composition, underlined.
    if let Some((k, from, to)) = parts.composition {
        let layout = &parts.paragraphs[k].layout;
        highlight(editor, &mut out, k, layout, from..to, ink, true);
    }
    if let Some((x, top, bottom)) = parts.caret {
        out.items.push(Item::Fill {
            path: Path::rect(x - PIXEL / 2, top, PIXEL * 3 / 2, bottom - top),
            color: ink,
        });
    }
    out
}

/// `item` as the draft shows it on the desk: text in the look's ink, and
/// tints, bars and rules in its colours, so it reads in light and dark.
fn on_desk(item: Item, res: &crate::Resources, look: &Look) -> Item {
    let blocks = &res.sheet.blocks;
    let rgb = |(r, g, b): (u8, u8, u8)| Color::rgb(r, g, b);
    let recolor = |color: Color| {
        if color == Color::BLACK || color == rgb(blocks.rule_color) {
            look.ink
        } else if color == rgb(blocks.link_color) {
            look.accent
        } else if color == rgb(blocks.code_tint) {
            look.tint
        } else if color == rgb(blocks.quote_color) || color == rgb(blocks.hairline_color) {
            look.line
        } else {
            color
        }
    };
    match item {
        Item::Glyphs(mut g) => {
            g.color = recolor(g.color);
            Item::Glyphs(g)
        }
        Item::Fill { path, color } => Item::Fill {
            path,
            color: recolor(color),
        },
        Item::Stroke { path, width, color } => Item::Stroke {
            path,
            width,
            color: recolor(color),
        },
        other => other,
    }
}

/// Fills `range` of paragraph `k`'s text, or underlines it.
fn highlight(
    editor: &Editor,
    out: &mut Page,
    k: usize,
    layout: &text::Layout,
    range: std::ops::Range<usize>,
    color: Color,
    underline: bool,
) {
    for rect in layout.selection(range) {
        let Some(line) = layout.lines.iter().position(|l| l.top == rect.y) else {
            continue;
        };
        let (x, y) = editor.line_origin_of(k, line);
        let top = y + rect.y;
        let path = if underline {
            Path::rect(x + rect.x, top + rect.height - 2 * PIXEL, rect.width, PIXEL)
        } else {
            Path::rect(x + rect.x, top, rect.width, rect.height)
        };
        out.items.push(Item::Fill { path, color });
    }
}
