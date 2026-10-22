//! `fetch-fonts`: downloads the fonts `spec/text/fonts.ron` lists into
//! `fonts/`, and the hyphenation patterns `spec/text/hyphenation.ron`
//! lists into `hyphenation/`, and checks each against its hash.
//!
//! ```text
//! fetch-fonts           fetch what's missing or wrong, check the rest
//! fetch-fonts --check   only check; fail if anything is missing or wrong
//! ```
//!
//! It downloads with `curl`, which Linux, macOS and Windows all have. A font
//! that comes only in a zip archive is taken out of it; an archive several
//! fonts come from is downloaded once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};
use testkit::fonts::{self, FontFile};
use testkit::hyphenation;

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
    let fetched = fetch_all(&fonts::dir(), &fonts::manifest()?.files, check_only)?;
    println!(
        "{}: the bundled fonts, checked ({fetched} fetched)",
        fonts::dir().display()
    );
    let dir = hyphenation::dir();
    let fetched = fetch_all(&dir, &hyphenation::manifest()?.files, check_only)?;
    println!(
        "{}: the hyphenation patterns, checked ({fetched} fetched)",
        dir.display()
    );
    Ok(())
}

/// Fetches into `dir` those of `files` that are missing or wrong, or with
/// `check_only` fails if any is. Returns how many it fetched.
fn fetch_all(dir: &Path, files: &[FontFile], check_only: bool) -> anyhow::Result<usize> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut fetched = 0;
    // Archives downloaded so far, by URL.
    let mut archives: HashMap<String, PathBuf> = HashMap::new();
    for file in files {
        let path = dir.join(&file.file);
        if matches(&path, file)? {
            continue;
        }
        if check_only {
            bail!("{} is missing or differs from its hash", path.display());
        }
        match &file.member {
            None => fetch(&path, file)?,
            Some(member) => {
                let archive = match archives.get(&file.url) {
                    Some(archive) => archive.clone(),
                    None => {
                        let archive = dir.join(format!(".archive-{}.zip", archives.len()));
                        download(&file.url, &archive)?;
                        archives.insert(file.url.clone(), archive.clone());
                        archive
                    }
                };
                extract(&archive, member, &path, file)?;
            }
        }
        fetched += 1;
    }
    for archive in archives.values() {
        std::fs::remove_file(archive).ok();
    }
    Ok(fetched)
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
    download(&file.url, &partial)?;
    let bytes =
        std::fs::read(&partial).with_context(|| format!("reading {}", partial.display()))?;
    let got = sha256(&bytes);
    if got != file.sha256 {
        std::fs::remove_file(&partial).ok();
        bail!(
            "{} has the hash {got}, not {} as its spec says",
            file.url,
            file.sha256
        );
    }
    std::fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Takes `member` out of the zip archive at `archive` as `file`, into
/// `path`, if its hash is the one `file` gives.
fn extract(archive: &Path, member: &str, path: &Path, file: &FontFile) -> anyhow::Result<()> {
    println!("taking {} out of {}", file.file, file.url);
    let reader =
        std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(reader).with_context(|| format!("reading {}", file.url))?;
    let mut entry = zip
        .by_name(member)
        .with_context(|| format!("{} has no {member}", file.url))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)
        .with_context(|| format!("unpacking {member}"))?;
    let got = sha256(&bytes);
    if got != file.sha256 {
        bail!(
            "{member} in {} has the hash {got}, not {} as its spec says",
            file.url,
            file.sha256
        );
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Downloads `url` to `path`.
fn download(url: &str, path: &Path) -> anyhow::Result<()> {
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
        .arg(path)
        .arg(url)
        .status()
        .context("running curl")?;
    if !status.success() {
        bail!("curl couldn't fetch {url}: {status}");
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
