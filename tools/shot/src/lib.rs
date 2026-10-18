//! `shot`'s scenes: named things to render, each to an [`Image`].
//!
//! Every render a person can look at and approve is a scene here, so the
//! `shot` tool, agents and the golden test (`tests/golden.rs`) all draw it
//! the same way. M1 starts with one scene that checks the harness itself;
//! the stages after it add documents, pages and chrome.
//!
//! Invariants:
//!
//! - Scene names are unique, and usable as file names: lowercase letters,
//!   digits and `-`.
//! - A scene renders the same pixels every time, on every platform.

use testkit::Image;

/// A named render.
pub struct Scene {
    /// The scene's name, which its PNG and golden record are named after.
    pub name: &'static str,
    /// What it shows, in a line.
    pub description: &'static str,
    /// Draws it.
    pub render: fn() -> Image,
}

/// Every scene, in the order `shot --list` shows them.
pub fn scenes() -> &'static [Scene] {
    &[Scene {
        name: "filled-rect",
        description: "the harness's own check: a dark rectangle on white, 64 by 48",
        render: filled_rect,
    }]
}

/// The scene named `name`.
pub fn find(name: &str) -> Option<&'static Scene> {
    scenes().iter().find(|scene| scene.name == name)
}

fn filled_rect() -> Image {
    let mut image = Image::filled(64, 48, [255, 255, 255, 255]);
    image.fill_rect(16, 16, 32, 16, [32, 48, 96, 255]);
    image
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
    fn scenes_render_the_same_twice() {
        for scene in scenes() {
            assert_eq!((scene.render)(), (scene.render)(), "{}", scene.name);
        }
    }
}
