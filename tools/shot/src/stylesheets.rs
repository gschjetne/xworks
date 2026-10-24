//! The stylesheets that come with xWorks (M2's stage 9), each shown in one
//! scene: the same sample document (`spec/documents/m2-stylesheets.ron`)
//! made in it, its first two pages side by side, for the owner to approve
//! each sheet's look as a golden.

use scene::Page;

use crate::pages::{fonts, sheet, spec, stylesheet};

/// The sample document made in stylesheet `name`, in its own automatic
/// pages.
pub(crate) fn stylesheet_pages(name: &str) -> canvas::Pages {
    let sheet_spec = stylesheet(name);
    let blocks: Vec<story::BlockSpec> =
        ron::from_str(&spec("documents", "m2-stylesheets")).expect("a document");
    let document = story::build(&sheet_spec, &blocks);
    let program = story::StyleSheet::from_spec(&stylesheet("neutral"));
    let resolved = story::StyleSheet::of(&document, &program);
    canvas::automatic(&document, &resolved, &resolved.page, fonts())
}

/// The sample's first two pages in stylesheet `name`, side by side.
fn first_pages(name: &str) -> Page {
    let mut pages = stylesheet_pages(name).pages;
    pages.truncate(2);
    sheet(&pages)
}

/// The sample in Neutral.
pub(crate) fn neutral() -> Page {
    first_pages("neutral")
}

/// The sample in Book.
pub(crate) fn book() -> Page {
    first_pages("book")
}

/// The sample in Literary.
pub(crate) fn literary() -> Page {
    first_pages("literary")
}

/// The sample in Modern.
pub(crate) fn modern() -> Page {
    first_pages("modern")
}

/// The sample in Technical.
pub(crate) fn technical() -> Page {
    first_pages("technical")
}

/// The sample in Manuscript.
pub(crate) fn manuscript() -> Page {
    first_pages("manuscript")
}
