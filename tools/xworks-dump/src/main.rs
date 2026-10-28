//! `xworks-dump`: any document in its canonical text form, for tests and
//! diffs (docs/ARCHITECTURE.md, section 6).
//!
//! ```text
//! xworks-dump FILE.xw
//! ```
//!
//! It prints what the document is, the document as it stands, its log of
//! transactions, its named versions, and the fonts and pictures it
//! carries. Two files with equal documents and histories dump identically.

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("usage: xworks-dump FILE.xw")?;
    let dump = store::dump(std::path::Path::new(&path))
        .with_context(|| format!("dumping {}", path.to_string_lossy()))?;
    print!("{dump}");
    Ok(())
}
