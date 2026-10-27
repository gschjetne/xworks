//! The sample documents the scenes draw (`spec/documents/`), written as
//! `.xw` files a person can open in xWorks, edit and try things on, as
//! `shot --xw DIR` writes them: the walking skeleton, the blocks of a
//! story, pictures in a story, and the stylesheets' sample in each sheet
//! that comes with xWorks, the one each `stylesheet-*` scene shows.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::pages::{neutral, spec, stylesheet};

/// The stylesheets the sample is written in, by file and as they're named.
const SHEETS: [(&str, &str); 6] = [
    ("neutral", "Neutral"),
    ("book", "Book"),
    ("literary", "Literary"),
    ("modern", "Modern"),
    ("technical", "Technical"),
    ("manuscript", "Manuscript"),
];

/// Writes each sample document into `dir`, which it makes if need be, as
/// an `.xw` file named after its first heading, and the stylesheets' sample
/// after its sheet too. A file already there is left as it is, as it may
/// have been edited. Returns the files written, then those left, in the
/// order above.
pub fn write_samples(dir: &Path) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut documents = Vec::new();
    let paragraphs: Vec<(String, Vec<(String, doc::Props)>)> =
        ron::from_str(&spec("documents", "m1-sample")).context("spec/documents/m1-sample.ron")?;
    documents.push((
        "A walking skeleton".to_string(),
        story::document(&neutral(), &paragraphs),
        Vec::new(),
    ));
    let blocks: Vec<story::BlockSpec> =
        ron::from_str(&spec("documents", "m2-blocks")).context("spec/documents/m2-blocks.ron")?;
    documents.push((
        "Blocks of a story".to_string(),
        story::build(&neutral(), &blocks),
        Vec::new(),
    ));
    // The pictures' sample names its pictures "landscape" and "heart",
    // which a file keeps by the IDs of their bytes, so it names those;
    // "lost" stays, as a picture the file lacks.
    let pictures = [
        ("landscape", crate::drawing::photograph_file()),
        ("heart", crate::drawing::icon_file()),
    ];
    let mut text = spec("documents", "m2-pictures");
    for (name, file) in &pictures {
        text = text.replace(
            &format!("\"picture\": Text(\"{name}\")"),
            &format!("\"picture\": Text(\"{}\")", store::image_id(file)),
        );
    }
    let blocks: Vec<story::BlockSpec> =
        ron::from_str(&text).context("spec/documents/m2-pictures.ron")?;
    documents.push((
        "Pictures in a story".to_string(),
        story::build(&neutral(), &blocks),
        pictures.iter().map(|(_, file)| file.clone()).collect(),
    ));
    let blocks: Vec<story::BlockSpec> = ron::from_str(&spec("documents", "m2-stylesheets"))
        .context("spec/documents/m2-stylesheets.ron")?;
    for (file, name) in SHEETS {
        documents.push((
            format!("The Shape of a Page ({name})"),
            story::build(&stylesheet(file), &blocks),
            Vec::new(),
        ));
    }

    let (mut written, mut left) = (Vec::new(), Vec::new());
    for (name, document, pictures) in documents {
        let path = dir.join(format!("{name}.xw"));
        if path.exists() {
            left.push(path);
            continue;
        }
        let mut store = store::Store::create(&path, &document, "story")
            .with_context(|| format!("writing {}", path.display()))?;
        for file in &pictures {
            store
                .add_image("png", file)
                .with_context(|| format!("keeping a picture in {}", path.display()))?;
        }
        written.push(path);
    }
    Ok((written, left))
}
