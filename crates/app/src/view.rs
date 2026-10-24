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
            // A soft shadow, as the mockups have it, and the page. The
            // page covers most of the shadow, which isn't drawn: only a
            // rim reaching a little under the page's edge, so the page
            // is anti-aliased over shadow there as over a whole one.
            let r = 2 * PIXEL;
            let under = Rect::new(x, y, style.width, style.height).inset(r + PIXEL);
            for (dx, dy, dw) in [(-PIXEL, 0, 2 * PIXEL), (0, PIXEL, 0)] {
                let shadow = Rect::new(x + dx, y + dy, style.width + dw, style.height);
                out.items.push(Item::Fill {
                    path: toolkit::ring(shadow, r, under),
                    color: look.shadow,
                });
            }
            out.items.push(Item::Fill {
                path: Path::rect(x, y, style.width, style.height),
                color: look.page,
            });
        }
        // A draft is one long column: only its pieces in view are drawn.
        let mut pieces = &parts.pieces[n][..];
        if parts.draft {
            let body = y + style.top;
            let bottom = |p: &canvas::Piece| {
                body + p.y
                    + parts.blocks[p.block].lines[p.lines.clone()]
                        .iter()
                        .sum::<i64>()
            };
            let first = pieces.partition_point(|p| bottom(p) < view_top);
            let last = pieces.partition_point(|p| body + p.y <= view_bottom);
            pieces = &pieces[first..last.max(first)];
        }
        let painted = canvas::paint_page(
            &editor.sheet().blocks,
            canvas::Poured {
                paragraphs: parts.paragraphs,
                units: parts.units,
                blocks: parts.blocks,
                pictures: parts.pictures,
            },
            pieces,
            Point::new(x + style.left, y + style.top),
            fonts,
            None,
        );
        if parts.draft {
            out.items.extend(
                painted
                    .into_iter()
                    .map(|item| on_desk(item, editor.sheet(), look)),
            );
        } else {
            out.items.extend(painted);
        }
    }

    // Direct formatting, tinted, where the person asked to see it: text
    // with its own character values, and a bar beside each paragraph
    // with its own paragraph values.
    if editor.shows_formatting() {
        for (k, p) in parts.paragraphs.iter().enumerate() {
            let lines = &p.layout.lines;
            let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
                continue;
            };
            let (x, top) = editor.line_origin_of(k, 0);
            let (_, bottom) = editor.line_origin_of(k, lines.len() - 1);
            let bottom = bottom + last.top - first.top + last.height;
            if bottom < view_top || top > view_bottom {
                continue;
            }
            let (ranges, whole) = editor.direct_formatting(k);
            for range in ranges {
                highlight(editor, &mut out, k, &p.layout, range, look.formatting, false);
            }
            if whole {
                let bar = x - p.margin - p.inset - 6 * PIXEL;
                out.items.push(Item::Fill {
                    path: Path::rect(bar, top + first.top, 2 * PIXEL, bottom - top - first.top),
                    color: look.formatting,
                });
            }
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
    if let Some((x, top, bottom)) = parts.caret.filter(|_| look.caret) {
        out.items.push(Item::Fill {
            path: Path::rect(x - PIXEL / 2, top, PIXEL * 3 / 2, bottom - top),
            color: ink,
        });
    }
    out
}

/// `item` as the draft shows it on the desk: text in the look's ink, and
/// tints, bars and rules in its colours, so it reads in light and dark.
fn on_desk(item: Item, sheet: &story::StyleSheet, look: &Look) -> Item {
    let rgb = |(r, g, b): (u8, u8, u8)| Color::rgb(r, g, b);
    let blocks = &sheet.blocks;
    // Links are in the colour the stylesheet's link mark gives them.
    let link = sheet.mark(4).color.map(rgb);
    let recolor = |color: Color| {
        if color == Color::BLACK
            || color == rgb(sheet.defaults.color)
            || color == rgb(blocks.rule_color)
        {
            look.ink
        } else if Some(color) == link {
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
