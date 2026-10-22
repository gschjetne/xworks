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

mod pages;

use testkit::Image;

pub use pages::{
    document_pages, document_sheet, editor_menu_page, editor_page, exit_pages, exit_sheet, fonts,
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
            name: "text",
            description: "paragraphs in every M1 script, bold, italic and slanted, at 144 pixels per inch",
            ppi: 144.0,
            page: text_page,
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
