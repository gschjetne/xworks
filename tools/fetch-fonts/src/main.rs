//! `fetch-fonts`: downloads the fonts `spec/text/fonts.ron` lists into
//! `fonts/`, and checks each against its hash.
//!
//! ```text
//! fetch-fonts           fetch what's missing or wrong, check the rest
//! fetch-fonts --check   only check; fail if anything is missing or wrong
//! ```
//!
//! It downloads with `curl`, which Linux, macOS and Windows all have.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};
use testkit::fonts::{self, FontFile};

fn main() -> anyhow::Result<()> {
    let check_only = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some("-h" | "--help") => {
            println!("usage: fetch-fonts [--check]");
            return Ok(());
        }
        Some(arg) => bail!("unknown argument {arg}; try --help"),
    };
    let dir = fonts::dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut fetched = 0;
    for file in fonts::manifest()?.files {
        let path = dir.join(&file.file);
        if matches(&path, &file)? {
            continue;
        }
        if check_only {
            bail!("{} is missing or differs from its hash", path.display());
        }
        fetch(&path, &file)?;
        fetched += 1;
    }
    println!(
        "{}: the bundled fonts, checked ({fetched} fetched)",
        dir.display()
    );
    Ok(())
}

/// Whether `path` holds `file`'s bytes.
fn matches(path: &Path, file: &FontFile) -> anyhow::Result<bool> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(sha256(&bytes) == file.sha256),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Downloads `file` to `path`, through a temporary file so a failed or
/// wrong download never takes its place.
fn fetch(path: &Path, file: &FontFile) -> anyhow::Result<()> {
    println!("fetching {}", file.file);
    let partial = path.with_extension("partial");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "3",
        ])
        .arg("--output")
        .arg(&partial)
        .arg(&file.url)
        .status()
        .context("running curl")?;
    if !status.success() {
        bail!("curl couldn't fetch {}: {status}", file.url);
    }
    let bytes =
        std::fs::read(&partial).with_context(|| format!("reading {}", partial.display()))?;
    let got = sha256(&bytes);
    if got != file.sha256 {
        std::fs::remove_file(&partial).ok();
        bail!(
            "{} has the hash {got}, not {} as spec/text/fonts.ron says",
            file.url,
            file.sha256
        );
    }
    std::fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
