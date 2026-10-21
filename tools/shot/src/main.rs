//! `shot`: renders scenes to PNG, so people and agents can look at them,
//! and records approved renders as goldens (spec/golden/README.md).
//!
//! ```text
//! shot [SCENE...]                     render scenes to target/shot/SCENE.png
//! shot --approve "NAME" [SCENE...]    also record them in spec/golden/
//! shot --pdf [SCENE...]               write them as pages of target/shot/scenes.pdf
//! shot --list                         list the scenes
//! ```
//!
//! With no scenes named, it renders every scene. Only a person may approve.

use anyhow::{Context, bail};
use testkit::golden::{self, Golden};

fn main() -> anyhow::Result<()> {
    let mut approver = None;
    let mut pdf = false;
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
            "--pdf" => pdf = true,
            "-h" | "--help" => {
                println!("usage: shot [--list] [--pdf] [--approve NAME] [SCENE...]");
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

    let out = testkit::workspace_root().join("target/shot");
    if pdf {
        if approver.is_some() {
            bail!("goldens are images; approve without --pdf");
        }
        let pages: Vec<_> = scenes.iter().map(|scene| (scene.page)()).collect();
        let bytes = render_pdf::pdf(&pages, Some("xWorks scenes"))?;
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
