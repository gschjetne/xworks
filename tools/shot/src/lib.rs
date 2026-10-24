//! `shot`'s scenes: named pages to render, to an [`Image`] or to PDF.
//!
//! Every render a person can look at and approve is a scene here, so the
//! `shot` tool, agents and the golden test (`tests/golden.rs`) all draw it
//! the same way. Scenes are display lists (`scene`), drawn by `render-cpu`
//! at its portable level, whose pixels are the same on every platform.
//!
//! Invariants:
//!
//! - Scene names are unique, and usable as file names: lowercase letters,
//!   digits and `-`.
//! - A scene renders the same pixels every time, on every platform.

mod chrome;
mod drawing;
mod mockups;
mod pages;
mod stylesheets;
mod typeset;

use testkit::Image;

pub use typeset::typeset_page;

pub use pages::{
    blocks_pages, blocks_sheet, document_pages, document_sheet, editor_lists_page,
    editor_menu_page, editor_page, exit_pages, exit_sheet, fonts, pictures_pages, pictures_sheet,
    shapes_page, text_page,
};

/// A named page, and the resolution it's drawn at.
pub struct Scene {
    /// The scene's name, which its PNG and golden record are named after.
    pub name: &'static str,
    /// What it shows, in a line.
    pub description: &'static str,
    /// Pixels per inch to draw it at.
    pub ppi: f64,
    /// Makes its page.
    pub page: fn() -> scene::Page,
    /// For a scene that shows several pages on one sheet: the pages
    /// themselves and their structure, for PDF.
    pub pages: Option<fn() -> canvas::Pages>,
}

impl Scene {
    /// Draws it, portably.
    pub fn render(&self) -> Image {
        draw(&(self.page)(), self.ppi)
    }
}

/// Every scene, in the order `shot --list` shows them.
pub fn scenes() -> &'static [Scene] {
    &[
        Scene {
            name: "filled-rect",
            description: "the harness's own check: a dark rectangle on white, 64 by 48",
            ppi: 96.0,
            page: filled_rect,
            pages: None,
        },
        Scene {
            name: "shapes",
            description: "fills, strokes, curves and transparency, at 96 pixels per inch",
            ppi: 96.0,
            page: shapes_page,
            pages: None,
        },
        Scene {
            name: "document",
            description: "the sample document in automatic A4 pages, side by side, at 48 pixels per inch",
            ppi: 48.0,
            page: document_sheet,
            pages: Some(document_pages),
        },
        Scene {
            name: "editor",
            description: "the editor's window after the tour session, a word selected",
            ppi: 96.0,
            page: editor_page,
            pages: None,
        },
        Scene {
            name: "editor-menu",
            description: "the editor's window with the Format menu open",
            ppi: 96.0,
            page: editor_menu_page,
            pages: None,
        },
        Scene {
            name: "exit-pages",
            description: "the pages M1's exit session types, undoes, redoes and reopens, side by side, at 48 pixels per inch",
            ppi: 48.0,
            page: exit_sheet,
            pages: Some(exit_pages),
        },
        Scene {
            name: "mockup-pages",
            description: "M2's chrome, mocked up: a document on pages, in xWorks' own title bar, with the ruler and the inspector",
            ppi: 96.0,
            page: mockups::pages_own_title,
            pages: None,
        },
        Scene {
            name: "mockup-pages-dark",
            description: "M2's chrome, mocked up: the same, dark, its page still white",
            ppi: 96.0,
            page: mockups::pages_dark,
            pages: None,
        },
        Scene {
            name: "mockup-draft-dark",
            description: "M2's chrome, mocked up: a Markdown file in the draft view, dark",
            ppi: 96.0,
            page: mockups::draft_dark,
            pages: None,
        },
        Scene {
            name: "mockup-palette",
            description: "M2's chrome, mocked up: the command palette, open",
            ppi: 96.0,
            page: mockups::palette,
            pages: None,
        },
        Scene {
            name: "chrome-pages",
            description: "M2's chrome, built: the sample on pages, with the ruler and the inspector, as mockup-pages shows it",
            ppi: 96.0,
            page: chrome::pages_light,
            pages: None,
        },
        Scene {
            name: "chrome-pages-dark",
            description: "M2's chrome, built: the same, dark, as mockup-pages-dark shows it",
            ppi: 96.0,
            page: chrome::pages_dark,
            pages: None,
        },
        Scene {
            name: "chrome-draft-dark",
            description: "M2's chrome, built: the sample as a draft, dark, as mockup-draft-dark shows it",
            ppi: 96.0,
            page: chrome::draft_dark,
            pages: None,
        },
        Scene {
            name: "chrome-palette",
            description: "M2's chrome, built: the command palette, open, as mockup-palette shows it",
            ppi: 96.0,
            page: chrome::palette,
            pages: None,
        },
        Scene {
            name: "ruler-centimetres",
            description: "the ruler over the sample on pages, in centimetres: marked from the page's one edge to the other, numbered out from the column's start",
            ppi: 96.0,
            page: chrome::ruler_centimetres,
            pages: None,
        },
        Scene {
            name: "ruler-inches",
            description: "the same, in inches, to their eighths",
            ppi: 96.0,
            page: chrome::ruler_inches,
            pages: None,
        },
        Scene {
            name: "ruler-picas",
            description: "the same, in picas, numbered every six, with every three, every one and every six points ticked",
            ppi: 96.0,
            page: chrome::ruler_picas,
            pages: None,
        },
        Scene {
            name: "ruler-paper-edge",
            description: "the same, in centimetres, numbered from the paper's edge, as page layout programs number it",
            ppi: 96.0,
            page: chrome::ruler_paper_edge,
            pages: None,
        },
        Scene {
            name: "markdown-draft",
            description: "a Markdown file as it opens, in the draft view: every block and mark Markdown holds, a picture it links to, and what it keeps as written",
            ppi: 96.0,
            page: chrome::markdown_light,
            pages: None,
        },
        Scene {
            name: "markdown-draft-dark",
            description: "the same, dark",
            ppi: 96.0,
            page: chrome::markdown_dark,
            pages: None,
        },
        Scene {
            name: "blocks",
            description: "a document with every kind of block, lists, quotes, code and a table, and Markdown's marks, in automatic A4 pages, at 96 pixels per inch",
            ppi: 96.0,
            page: blocks_sheet,
            pages: Some(blocks_pages),
        },
        Scene {
            name: "editor-lists",
            description: "the editor's window after the lists session: lists typed, nested and numbered, closed and opened again",
            ppi: 96.0,
            page: editor_lists_page,
            pages: None,
        },
        Scene {
            name: "text",
            description: "paragraphs in every M1 script, bold, italic and slanted, at 144 pixels per inch",
            ppi: 144.0,
            page: text_page,
            pages: None,
        },
        Scene {
            name: "images",
            description: "a photograph large and from its smaller levels, an icon drawn large, a veil over text, at 96 pixels per inch",
            ppi: 96.0,
            page: drawing::images_page,
            pages: None,
        },
        Scene {
            name: "transforms",
            description: "text turned, scaled and mirrored, a picture in a round clip, a star clipping stripes, at 144 pixels per inch",
            ppi: 144.0,
            page: drawing::transforms_page,
            pages: None,
        },
        Scene {
            name: "decorations",
            description: "underlines and lines through text in several fonts, sizes and scripts, at 144 pixels per inch",
            ppi: 144.0,
            page: drawing::decorations_page,
            pages: None,
        },
        Scene {
            name: "pictures",
            description: "a document with pictures in its text: small and larger in a line, a figure scaled to its paragraph, in a list, a table's cell, and one the file lacks, at 96 pixels per inch",
            ppi: 96.0,
            page: pictures_sheet,
            pages: Some(pictures_pages),
        },
        Scene {
            name: "stylesheet-neutral",
            description: "the stylesheets' sample in Neutral, M2's look: Source Serif 4 and Inter, its first two pages, at 96 pixels per inch",
            ppi: 96.0,
            page: stylesheets::neutral,
            pages: None,
        },
        Scene {
            name: "stylesheet-book",
            description: "the stylesheets' sample in Book: EB Garamond on A5, justified and hyphenated, its first two pages, at 96 pixels per inch",
            ppi: 96.0,
            page: stylesheets::book,
            pages: None,
        },
        Scene {
            name: "stylesheet-literary",
            description: "the stylesheets' sample in Literary: Alegreya, ragged, with headings in its small capitals, its first two pages, at 96 pixels per inch",
            ppi: 96.0,
            page: stylesheets::literary,
            pages: None,
        },
        Scene {
            name: "stylesheet-modern",
            description: "the stylesheets' sample in Modern: Inter throughout, its first two pages, at 96 pixels per inch",
            ppi: 96.0,
            page: stylesheets::modern,
            pages: None,
        },
        Scene {
            name: "stylesheet-technical",
            description: "the stylesheets' sample in Technical: Source Serif 4, numbered headings, JetBrains Mono, its first two pages, at 96 pixels per inch",
            ppi: 96.0,
            page: stylesheets::technical,
            pages: None,
        },
        Scene {
            name: "stylesheet-manuscript",
            description: "the stylesheets' sample in Manuscript: double-spaced, for submissions, its first two pages, at 96 pixels per inch",
            ppi: 96.0,
            page: stylesheets::manuscript,
            pages: None,
        },
        Scene {
            name: "typeset",
            description: "text set well: one text ragged and justified, hyphenated and justified paragraphs in six languages, Japanese and Hebrew justified, and tab stops, at 144 pixels per inch",
            ppi: 144.0,
            page: typeset_page,
            pages: None,
        },
    ]
}

/// The scene named `name`.
pub fn find(name: &str) -> Option<&'static Scene> {
    scenes().iter().find(|scene| scene.name == name)
}

/// `page` drawn at `ppi`, portably.
pub fn draw(page: &scene::Page, ppi: f64) -> Image {
    let image = render_cpu::render(page, ppi, render_cpu::Level::Portable);
    Image::from_rgba(image.width, image.height, image.pixels).expect("a whole image")
}

fn filled_rect() -> scene::Page {
    // 64 by 48 pixels at 96 to the inch.
    let px = text::units::PIXEL;
    let mut page = scene::Page::new(64 * px, 48 * px);
    page.fill_background(scene::Color::WHITE);
    page.items.push(scene::Item::Fill {
        path: scene::Path::rect(16 * px, 16 * px, 32 * px, 16 * px),
        color: scene::Color::rgb(32, 48, 96),
    });
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_fit_file_names() {
        let mut names: Vec<_> = scenes().iter().map(|scene| scene.name).collect();
        for name in &names {
            assert!(
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name:?}"
            );
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), scenes().len());
    }

    #[test]
    fn the_fastest_level_draws_close_to_the_portable_one() {
        // Stage 3 measured vello_cpu's SIMD and scalar output within a few
        // levels of each other (prototypes/renderers/README.md).
        for scene in scenes() {
            let page = (scene.page)();
            let portable = render_cpu::render(&page, scene.ppi, render_cpu::Level::Portable);
            let fastest = render_cpu::render(&page, scene.ppi, render_cpu::Level::Fastest);
            let worst = portable
                .pixels
                .iter()
                .zip(&fastest.pixels)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert!(worst <= 8, "{}: {worst} levels apart", scene.name);
        }
    }

    #[test]
    fn scenes_render_the_same_twice() {
        for scene in scenes() {
            assert_eq!(scene.render(), scene.render(), "{}", scene.name);
        }
    }
}
