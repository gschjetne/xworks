//! `shot`: renders scenes to PNG, so people and agents can look at them,
//! and records approved renders as goldens (spec/golden/README.md).
//!
//! ```text
//! shot [SCENE...]                     render scenes to target/shot/SCENE.png
//! shot --approve "NAME" [SCENE...]    also record them in spec/golden/
//! shot --pdf [SCENE...]               write them as pages of target/shot/scenes.pdf,
//!                                     a sheet of pages as its pages
//! shot --hashes [SCENE...]            print their golden hashes, to compare platforms
//! shot --diff SCENE OTHER             how two scenes of one size differ: how many
//!                                     pixels, and target/shot/diff-SCENE-OTHER.png
//!                                     showing them in red over a faded SCENE
//! shot --list                         list the scenes
//! shot --xw DIR                       write the sample documents the scenes draw
//!                                     as .xw files in DIR, to open in xWorks,
//!                                     leaving any already there
//! ```
//!
//! With no scenes named, it renders every scene. Only a person may approve.

use anyhow::{Context, bail};
use testkit::golden::{self, Golden};

fn main() -> anyhow::Result<()> {
    let mut approver = None;
    let mut pdf = false;
    let mut hashes = false;
    let mut names = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => {
                for scene in shot::scenes() {
                    println!("{:<24} {}", scene.name, scene.description);
                }
                return Ok(());
            }
            "--approve" => {
                let name = args.next().context("--approve needs the approver's name")?;
                if name.trim().is_empty() {
                    bail!("--approve needs the approver's name");
                }
                approver = Some(name);
            }
            "--xw" => {
                let dir = args.next().context("--xw needs a folder")?;
                let (written, left) = shot::write_samples(std::path::Path::new(&dir))?;
                for path in written {
                    println!("{}", path.display());
                }
                for path in left {
                    println!("{} (already there, left as it is)", path.display());
                }
                return Ok(());
            }
            "--pdf" => pdf = true,
            "--hashes" => hashes = true,
            "--diff" => {
                let a = args.next().context("--diff needs two scenes")?;
                let b = args.next().context("--diff needs two scenes")?;
                return diff(&a, &b);
            }
            "-h" | "--help" => {
                println!(
                    "usage: shot [--list] [--pdf | --hashes] [--approve NAME] [SCENE...]\n       shot --diff SCENE OTHER\n       shot --xw DIR"
                );
                return Ok(());
            }
            _ if arg.starts_with('-') => bail!("unknown option {arg}; try --help"),
            _ => names.push(arg),
        }
    }

    let scenes: Vec<_> = if names.is_empty() {
        shot::scenes().iter().collect()
    } else {
        names
            .iter()
            .map(|name| shot::find(name).with_context(|| format!("no scene {name:?}; try --list")))
            .collect::<anyhow::Result<_>>()?
    };

    if hashes {
        for scene in scenes {
            let image = scene.render();
            println!(
                "{:<16} {}x{} {}",
                scene.name,
                image.width(),
                image.height(),
                golden::hash(&image)
            );
        }
        return Ok(());
    }
    let out = testkit::workspace_root().join("target/shot");
    if pdf {
        if approver.is_some() {
            bail!("goldens are images; approve without --pdf");
        }
        // A scene of pages brings their structure; the PDF's structure is
        // its blocks, one after another.
        let mut pages = Vec::new();
        let mut structure = scene::Structure::default();
        for scene in &scenes {
            match scene.pages {
                Some(make) => {
                    let made = make();
                    let first = structure.append(&made.structure);
                    for mut page in made.pages {
                        for item in &mut page.items {
                            if let scene::Item::Glyphs(run) = item
                                && let Some(source) = &mut run.source
                            {
                                source.block += first;
                            }
                        }
                        pages.push(page);
                    }
                }
                None => pages.push((scene.page)()),
            }
        }
        let bytes = render_pdf::pdf(&pages, &structure, Some("xWorks scenes"))?;
        std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
        let path = out.join("scenes.pdf");
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        println!("{}", path.display());
        return Ok(());
    }
    for scene in scenes {
        let image = scene.render();
        let path = out.join(format!("{}.png", scene.name));
        image.write_png(&path)?;
        println!("{}", path.display());
        if let Some(approver) = &approver {
            let record = Golden::approve(scene.name, &image, approver);
            golden::write(&golden::dir(), &record)?;
            println!(
                "  approved: {}",
                golden::record_path(&golden::dir(), scene.name).display()
            );
        }
    }
    Ok(())
}

/// Compares scenes `a` and `b`, which must be one size: prints how many
/// pixels differ, more than a little and at all, and writes an image of
/// `a`, faded, with those that differ more than a little in red.
fn diff(a: &str, b: &str) -> anyhow::Result<()> {
    let find =
        |name: &str| shot::find(name).with_context(|| format!("no scene {name:?}; try --list"));
    let (first, second) = (find(a)?.render(), find(b)?.render());
    if (first.width(), first.height()) != (second.width(), second.height()) {
        bail!("{a} and {b} aren't one size");
    }
    let (w, h) = (first.width(), first.height());
    let mut out = testkit::Image::new(w, h);
    let (mut any, mut more) = (0u64, 0u64);
    for y in 0..h {
        for x in 0..w {
            let p = first.pixel(x, y).unwrap_or([0; 4]);
            let q = second.pixel(x, y).unwrap_or([0; 4]);
            let worst = p
                .iter()
                .zip(&q)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            any += u64::from(worst > 0);
            let shown = if worst > 16 {
                more += 1;
                [220, 30, 30, 255]
            } else {
                let grey = ((u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3) as u8;
                let faded = 160 + grey / 3;
                [faded, faded, faded, 255]
            };
            out.fill_rect(x, y, 1, 1, shown);
        }
    }
    let total = u64::from(w) * u64::from(h);
    let path = testkit::workspace_root()
        .join("target/shot")
        .join(format!("diff-{a}-{b}.png"));
    out.write_png(&path)?;
    let share = |n: u64| 100.0 * n as f64 / total as f64;
    println!(
        "{a} and {b}: {more} pixels ({:.2}%) differ by more than 16 levels, {any} ({:.2}%) at all",
        share(more),
        share(any)
    );
    println!("{}", path.display());
    Ok(())
}
