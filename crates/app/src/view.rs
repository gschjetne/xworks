//! Drawing a window: the pages in view, the selection, the caret and the
//! composition, then the chrome over them.

use scene::{Color, Item, Page, Path, Point};
use text::units::PIXEL;

use crate::Editor;

pub(crate) fn draw(editor: &Editor) -> Page {
    let res = editor.resources();
    let parts = editor.view_parts();
    let fonts = res.fonts();
    let style = &res.page;
    let mut out = Page::new(parts.width, parts.height);
    out.fill_background(res.theme.desk);

    // The pages in view, and their text.
    let (view_top, view_bottom) = (parts.top, parts.top + parts.view_height);
    for n in 0..parts.pages {
        let (x, y) = editor.origin_of_page(n);
        if y + style.height < view_top || y > view_bottom {
            continue;
        }
        out.items.push(Item::Fill {
            path: Path::rect(x + 2 * PIXEL, y + 2 * PIXEL, style.width, style.height),
            color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 45,
            },
        });
        out.items.push(Item::Fill {
            path: Path::rect(x, y, style.width, style.height),
            color: Color::WHITE,
        });
        out.items.extend(canvas::paint_page(
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
        ));
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
                res.theme.selection,
                false,
            );
        }
    }
    // The composition, underlined.
    if let Some((k, from, to)) = parts.composition {
        let layout = &parts.paragraphs[k].layout;
        highlight(editor, &mut out, k, layout, from..to, Color::BLACK, true);
    }
    if let Some((x, top, bottom)) = parts.caret {
        out.items.push(Item::Fill {
            path: Path::rect(x - PIXEL / 2, top, PIXEL * 3 / 2, bottom - top),
            color: Color::BLACK,
        });
    }

    // The chrome over everything.
    let theme = &res.theme;
    let (status, _) = toolkit::status_bar(
        fonts,
        theme,
        &parts.status,
        &parts.message,
        parts.width,
        parts.height,
    );
    out.items.extend(status);
    out.items
        .extend(parts.menu.draw(fonts, theme, parts.hover.as_ref()));
    if let Some((list, selected)) = &parts.list {
        out.items.extend(list.draw(fonts, theme, Some(*selected)));
    }
    out
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
