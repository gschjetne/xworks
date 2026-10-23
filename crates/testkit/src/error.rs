use std::path::PathBuf;

/// What can go wrong reading or writing test images and records.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A pixel buffer's length doesn't match its image's size.
    #[error("{len} bytes of pixels for a {width}×{height} image, which needs {expected}")]
    Size {
        /// The image's width.
        width: u32,
        /// The image's height.
        height: u32,
        /// The buffer's length.
        len: usize,
        /// The length the size needs.
        expected: usize,
    },
    /// A file couldn't be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// A golden record isn't valid RON, or isn't a record.
    #[error("{path}: {source}")]
    Record {
        /// The record's file.
        path: PathBuf,
        /// Why.
        source: Box<ron::error::SpannedError>,
    },
    /// A golden record's name doesn't match its file's name.
    #[error("{path}: records the image {name:?}, but the file is named for another")]
    Misnamed {
        /// The record's file.
        path: PathBuf,
        /// The name inside it.
        name: String,
    },
    /// A bundled font hasn't been fetched.
    #[error("{path} is missing; fetch the fonts with `cargo run -p fetch-fonts`")]
    MissingFont {
        /// Where it should be.
        path: PathBuf,
    },
    /// A hyphenation file hasn't been fetched.
    #[error("{path} is missing; fetch it with the fonts, `cargo run -p fetch-fonts`")]
    MissingHyphenation {
        /// Where it should be.
        path: PathBuf,
    },
    /// A file fetched by hash, such as CommonMark's examples, hasn't been
    /// fetched.
    #[error("{path} is missing; fetch it with the fonts, `cargo run -p fetch-fonts`")]
    MissingFetched {
        /// Where it should be.
        path: PathBuf,
    },
    /// A JSON file isn't what it should be.
    #[error("{path} isn't the JSON it should be")]
    Json {
        /// The file.
        path: PathBuf,
    },
    /// An image couldn't be encoded as PNG.
    #[error("encoding PNG: {0}")]
    Png(#[from] png::EncodingError),
    /// A PDF couldn't be read.
    #[error("reading the PDF: {0}")]
    Pdf(#[from] lopdf::Error),
}
