//! `budgets`: measures the performance budgets that apply to M1 on the
//! machine it runs on, and prints each against its budget
//! (`spec/budgets.ron`; docs/ARCHITECTURE.md, section 12; docs/ROADMAP.md,
//! M1 stage 10).
//!
//! ```text
//! budgets [--shell PATH] [--runs N] [--scale S]
//! ```
//!
//! - **Cold start** and **memory for an empty window**: it runs the shell
//!   (`--shell`, by default `xworks` beside this program) with
//!   `XWORKS_MEASURE_STARTUP`, from starting it to its first frame
//!   presented, and reads its resident memory then. Run a release build.
//!   Memory is judged on a real GPU; under WSL it's shown for
//!   information.
//! - **Opening a 500-page report**, **keystroke to glyph**, **scrolling**
//!   and **saving after a small edit**: in this process, with the app's
//!   window, its chrome and all, on a report it makes from the sample
//!   document. Frames are
//!   drawn as the shell draws them, in a window of the shell's size (times
//!   `--scale`, for a display that scales): with render-gpu on a hardware
//!   GPU, or render-cpu where the GPU is software or missing, into the
//!   window's canvas where the view changed, and the canvas copied to the
//!   frame. A frame counts until the GPU has finished it.
//!
//! - **A large Markdown file** (M2's stage 6), which has no budgets of its
//!   own yet: opening one as large as the report, first frame shown, and a
//!   keystroke in its middle, which writes the whole file back.
//!
//! Measure on a quiet machine. Each line gives the median and the worst,
//! and the parts of a frame where the time goes.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use app::{Event, Resources, Window};
use chrome::{Key, Modifiers};
use doc::{Op, Props, Run};
use serde::Deserialize;
use store::{Entry, Store};

/// `spec/budgets.ron`.
#[derive(Deserialize)]
struct Spec {
    report_pages: usize,
    budgets: Vec<Budget>,
}

#[derive(Deserialize)]
struct Budget {
    id: String,
    interaction: String,
    budget: Amount,
}

#[derive(Clone, Copy, Deserialize)]
enum Amount {
    Millis(f64),
    Megabytes(f64),
}

impl Amount {
    fn value(self) -> f64 {
        match self {
            Amount::Millis(v) | Amount::Megabytes(v) => v,
        }
    }

    fn show(self, v: f64) -> String {
        match self {
            Amount::Millis(_) if v >= 1000.0 => format!("{:.2} s", v / 1000.0),
            Amount::Millis(_) => format!("{v:.1} ms"),
            Amount::Megabytes(_) => format!("{v:.0} MB"),
        }
    }
}

/// What was measured for a budget: samples in its unit (milliseconds or
/// megabytes), and the parts of each, in milliseconds.
struct Measured {
    id: &'static str,
    samples: Vec<f64>,
    parts: Vec<(String, Vec<f64>)>,
    note: &'static str,
}

/// The shell's window, in logical pixels (shells/generic).
const WINDOW: (f64, f64) = (1000.0, 820.0);

fn main() -> anyhow::Result<()> {
    let mut shell: Option<PathBuf> = None;
    let mut runs = 5;
    let mut scale = 1.0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--shell" => shell = Some(args.next().context("--shell needs a path")?.into()),
            "--runs" => runs = args.next().context("--runs needs a number")?.parse()?,
            "--scale" => scale = args.next().context("--scale needs a number")?.parse()?,
            "-h" | "--help" => {
                println!("usage: budgets [--shell PATH] [--runs N] [--scale S]");
                return Ok(());
            }
            _ => bail!("unknown argument {arg}; try --help"),
        }
    }
    let root = root().context("can't find xWorks' spec/ and fonts/; set XWORKS_ROOT")?;
    let spec: Spec = ron::from_str(&std::fs::read_to_string(root.join("spec/budgets.ron"))?)
        .context("spec/budgets.ron")?;
    let scratch = std::env::temp_dir().join(format!("xworks-budgets-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    println!("budgets: on {}", machine());

    let mut measured = Vec::new();
    let shell = shell.unwrap_or_else(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        exe.with_file_name(if cfg!(windows) {
            "xworks.exe"
        } else {
            "xworks"
        })
    });
    match startup(&shell, &scratch, runs) {
        Ok(lines) => measured.extend(lines),
        Err(e) => println!("budgets: no cold start or memory: {e:#}"),
    }

    // Memory's parts, in this process: what loading the fonts and specs
    // takes, and opening the GPU.
    let resident = || resident_kb(std::process::id());
    let before = resident();
    let mac = cfg!(target_os = "macos");
    let res = Arc::new(Resources::load(&root, scratch.clone(), mac)?);
    let loaded = resident();
    let mut frames = Frames::new(scale);
    let opened = resident();
    if let (Some(memory), Some(before), Some(loaded), Some(opened)) = (
        measured.iter_mut().find(|m| m.id == "memory"),
        before,
        loaded,
        opened,
    ) {
        let mb = |kb: u64| vec![kb as f64 / 1024.0];
        memory.parts = vec![
            (
                "fonts and specs".to_string(),
                mb(loaded.saturating_sub(before)),
            ),
            ("GPU opened".to_string(), mb(opened.saturating_sub(loaded))),
        ];
    }
    let path = scratch.join("report.xw");
    let (paragraphs, pages) = report(&root, &res, &path, spec.report_pages)?;
    println!("budgets: a report of {pages} pages, {paragraphs} paragraphs");
    println!("budgets: frames drawn by {}", frames.by());
    measured.extend(in_editor(&res, &path, &mut frames, runs)?);
    measured.push(saving(&path)?);
    let markdown = markdown(&root, &res, &scratch, &mut frames, runs)?;

    println!();
    println!(
        "{:<44} {:>9} {:>9} {:>9} {:>9}",
        "", "budget", "median", "95th", "worst"
    );
    for budget in &spec.budgets {
        let Some(m) = measured.iter().find(|m| m.id == budget.id) else {
            println!(
                "{:<44} {:>9}  not measured",
                budget.interaction,
                budget.budget.show(budget.budget.value())
            );
            continue;
        };
        let amount = budget.budget;
        // Judged by the 95th percentile: a budget holds nearly every time.
        let p95 = percentile(&m.samples, 95);
        let worst = m.samples.iter().copied().fold(0.0, f64::max);
        // Memory is judged on a real GPU (project owner, 2026-10-24,
        // review 77nh3z47): under WSL, Mesa's software GPU takes most of
        // it, which no one using xWorks runs.
        let verdict = if budget.id == "memory" && std::env::var_os("WSL_DISTRO_NAME").is_some() {
            "for information: WSL's software GPU"
        } else if p95 <= amount.value() {
            "within"
        } else {
            "OVER"
        };
        println!(
            "{:<44} {:>9} {:>9} {:>9} {:>9}  {verdict}",
            budget.interaction,
            amount.show(amount.value()),
            amount.show(percentile(&m.samples, 50)),
            amount.show(p95),
            amount.show(worst),
        );
        if !m.parts.is_empty() {
            let parts: Vec<String> = m
                .parts
                .iter()
                .map(|(what, v)| format!("{what} {}", amount.show(percentile(v, 50))))
                .collect();
            println!("    parts, medians: {}", parts.join(", "));
        }
        if !m.note.is_empty() {
            println!("    {}", m.note);
        }
    }
    println!();
    println!(
        "{:<44} {:>9} {:>9} {:>9}",
        "No budget yet", "median", "95th", "worst"
    );
    for (what, m) in &markdown {
        let ms = |v: f64| format!("{v:.1} ms");
        let worst = m.samples.iter().copied().fold(0.0, f64::max);
        println!(
            "{what:<44} {:>9} {:>9} {:>9}",
            ms(percentile(&m.samples, 50)),
            ms(percentile(&m.samples, 95)),
            ms(worst)
        );
        if !m.parts.is_empty() {
            let parts: Vec<String> = m
                .parts
                .iter()
                .map(|(what, v)| format!("{what} {}", ms(percentile(v, 50))))
                .collect();
            println!("    parts, medians: {}", parts.join(", "));
        }
        if !m.note.is_empty() {
            println!("    {}", m.note);
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

/// A Markdown file of about a megabyte: the Markdown files of
/// `spec/markdown/files/` over and over, their front matter left out.
fn large_markdown(root: &Path) -> anyhow::Result<String> {
    let mut parts = Vec::new();
    for name in ["readme", "notes", "changelog", "guide"] {
        let text = std::fs::read_to_string(root.join(format!("spec/markdown/files/{name}.md")))?;
        let text = match text.strip_prefix("---\n") {
            Some(rest) => rest
                .split_once("\n---\n")
                .map_or(text.clone(), |(_, body)| body.to_string()),
            None => text,
        };
        parts.push(text);
    }
    let mut out = String::new();
    while out.len() < 1 << 20 {
        for part in &parts {
            out.push_str(part);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Opening a large Markdown file, as the report is opened, and typing in
/// its middle, each keystroke writing the whole file back.
fn markdown(
    root: &Path,
    res: &Arc<Resources>,
    scratch: &Path,
    frames: &mut Frames,
    runs: usize,
) -> anyhow::Result<Vec<(String, Measured)>> {
    let text = large_markdown(root)?;
    let path = scratch.join("large.md");
    std::fs::write(&path, &text)?;
    let window = || Event::Resize {
        width: WINDOW.0,
        height: WINDOW.1,
    };
    let mut opening = Vec::new();
    let mut parts: [Vec<f64>; 2] = Default::default();
    let mut blocks = 0;
    for _ in 0..runs {
        let began = Instant::now();
        let mut editor = Window::open(res.clone(), &path)?;
        editor.update(window());
        let opened = millis(began.elapsed());
        frames.forget();
        frames.draw(&editor)?;
        opening.push(millis(began.elapsed()));
        parts[0].push(opened);
        parts[1].push(millis(began.elapsed()) - opened);
        blocks = editor.editor().document().paragraphs().len();
    }
    let [read, draw] = parts;
    let open = Measured {
        id: "open-markdown",
        samples: opening,
        parts: vec![
            ("read and lay out".to_string(), read),
            ("first frame".to_string(), draw),
        ],
        note: "",
    };
    let mut editor = Window::open(res.clone(), &path)?;
    editor.update(window());
    let paragraphs = editor.editor().document().paragraphs().to_vec();
    let middle = paragraphs.len() / 2;
    let at = |editor: &Window| {
        let caret = editor.editor().selection().1.paragraph;
        paragraphs.iter().position(|&p| p == caret).unwrap_or(0)
    };
    while at(&editor) < middle {
        editor.update(Event::Key {
            key: Key::PageDown,
            modifiers: Modifiers::default(),
        });
    }
    frames.draw(&editor)?;
    let mut typing = Vec::new();
    let mut parts: [Vec<f64>; 3] = Default::default();
    for c in "The quick brown fox jumps over the lazy dog. ".chars() {
        typing.push(frame(
            &mut editor,
            frames,
            Event::Text(c.to_string()),
            &mut parts,
        )?);
    }
    let keystroke = Measured {
        id: "keystroke-markdown",
        samples: typing,
        parts: named(parts, "update and write the file"),
        note: "",
    };
    let size = format!("{:.1} MB, {blocks} paragraphs", text.len() as f64 / 1e6);
    Ok(vec![
        (format!("Open a Markdown file of {size}"), open),
        (
            "Keystroke in it, the file written back".to_string(),
            keystroke,
        ),
    ])
}

fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn percentile(samples: &[f64], p: usize) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
        .get(sorted.len().saturating_sub(1) * p / 100)
        .copied()
        .unwrap_or_default()
}

/// Where `spec/` and `fonts/` are, as the shell finds them.
fn root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("XWORKS_ROOT") {
        return Some(PathBuf::from(root));
    }
    let has = |dir: &Path| dir.join("spec/text/fonts.ron").is_file();
    [std::env::current_exe().ok(), std::env::current_dir().ok()]
        .into_iter()
        .flatten()
        .find_map(|start| start.ancestors().find(|d| has(d)).map(Path::to_path_buf))
}

/// A line about the machine: its system and processors.
fn machine() -> String {
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    format!(
        "{} {}, {threads} threads",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Cold start and memory: the shell run `runs` times to its first frame.
fn startup(shell: &Path, scratch: &Path, runs: usize) -> anyhow::Result<Vec<Measured>> {
    if !shell.is_file() {
        bail!(
            "{} isn't there; build it (cargo build --release -p xworks) or pass --shell",
            shell.display()
        );
    }
    let mut times = Vec::new();
    let mut memory = Vec::new();
    // The phases the shell reports, in its order, with the process's own
    // start first: until the shell's clock starts.
    let mut phases: Vec<(String, Vec<f64>)> = vec![("process".to_string(), Vec::new())];
    for run in 0..runs {
        let documents = scratch.join(format!("start-{run}"));
        std::fs::create_dir_all(&documents)?;
        let began = Instant::now();
        let mut child = Command::new(shell)
            .env("XWORKS_MEASURE_STARTUP", "1")
            .env("XWORKS_DOCUMENTS", &documents)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", shell.display()))?;
        let mut out = std::io::BufReader::new(child.stdout.take().context("its output")?);
        let mut line = String::new();
        out.read_line(&mut line)?;
        let elapsed = began.elapsed();
        let mut words = line.split_whitespace();
        let (Some("ready"), Some(Ok(own))) = (words.next(), words.next().map(str::parse::<f64>))
        else {
            let _ = child.kill();
            bail!("the shell said {line:?}, not that it was ready");
        };
        times.push(millis(elapsed));
        phases[0].1.push(millis(elapsed) - own / 1000.0);
        for word in words {
            let Some((phase, Ok(micros))) =
                word.split_once('=').map(|(p, v)| (p, v.parse::<f64>()))
            else {
                continue;
            };
            match phases.iter_mut().find(|(p, _)| p == phase) {
                Some((_, v)) => v.push(micros / 1000.0),
                None => phases.push((phase.to_string(), vec![micros / 1000.0])),
            }
        }
        // Memory once the renderer that stays draws: the GPU's, which
        // compiles while render-cpu draws the first frames.
        line.clear();
        out.read_line(&mut line)?;
        if !line.starts_with("settled") {
            let _ = child.kill();
            bail!("the shell said {line:?}, not that it had settled");
        }
        if let Some(kb) = resident_kb(child.id()) {
            memory.push(kb as f64 / 1024.0);
        }
        drop(child.stdin.take());
        child.wait()?;
    }
    let mut lines = vec![Measured {
        id: "cold-start",
        samples: times,
        parts: phases,
        note: "from starting the process to its first frame presented and done; \
               the phases as the shell times them, gpu-instance and gpu-device \
               on a thread alongside the others",
    }];
    if !memory.is_empty() {
        lines.push(Measured {
            id: "memory",
            samples: memory,
            parts: Vec::new(),
            note: "resident memory once the renderer that stays has drawn a frame; \
                   its parts measured in this process",
        });
    }
    Ok(lines)
}

/// A process's resident memory in kilobytes, if it can be read.
fn resident_kb(pid: u32) -> Option<u64> {
    if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        line.split_whitespace().nth(1)?.parse().ok()
    } else if cfg!(windows) {
        // "xworks.exe","1234","Console","1","123,456 K": the working set.
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let field = text.trim().rsplit("\",\"").next()?;
        let digits: String = field.chars().filter(char::is_ascii_digit).collect();
        digits.parse().ok()
    } else {
        let out = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
}

/// Makes the report at `path`: the sample document repeated until it fills
/// `pages` automatic pages. Returns its paragraphs and pages.
fn report(
    root: &Path,
    res: &Resources,
    path: &Path,
    pages: usize,
) -> anyhow::Result<(usize, usize)> {
    let sample = std::fs::read_to_string(root.join("spec/documents/m1-sample.ron"))?;
    let sample: Vec<(String, Vec<(String, Props)>)> = ron::from_str(&sample)?;
    // The sample holds Chinese, whose fonts load on demand.
    res.load_on_demand()?;
    let count = |copies: usize| {
        let all: Vec<_> = sample
            .iter()
            .cycle()
            .take(sample.len() * copies)
            .cloned()
            .collect();
        let document = story::document(&res.stylesheets[0], &all);
        let sheet = &res.program;
        let laid = canvas::automatic(&document, sheet, &sheet.page, res.fonts());
        (all, document, laid.pages.len())
    };
    // Twenty copies say how many pages a copy fills.
    let (_, _, twenty) = count(20);
    let mut copies = (pages * 20).div_ceil(twenty.max(1));
    loop {
        let (all, document, made) = count(copies);
        if made >= pages {
            Store::create(path, &document, "story")?;
            return Ok((all.len(), made));
        }
        copies += 1;
    }
}

/// What draws frames, as the shell does: the GPU if it's hardware, the CPU
/// otherwise, into a canvas where the view changed, and the canvas copied
/// to the frame.
struct Frames {
    gpu: Option<Gpu>,
    /// render-cpu, and the page its canvas shows.
    cpu: (render_cpu::Renderer, Option<scene::Page>),
    ppi: f64,
    size: (u16, u16),
}

/// A hardware GPU, its renderer, the window's canvas and a frame to copy
/// it to.
struct Gpu {
    device: render_gpu::Device,
    renderer: render_gpu::Renderer,
    canvas: render_gpu::Canvas,
    frame: wgpu::Texture,
}

impl Frames {
    /// Opens the GPU, if there's a hardware one, and makes the renderer.
    fn new(scale: f64) -> Frames {
        let size = (
            (WINDOW.0 * scale).round() as u16,
            (WINDOW.1 * scale).round() as u16,
        );
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let gpu = render_gpu::open(&instance, None)
            .filter(|d| d.adapter.get_info().device_type != wgpu::DeviceType::Cpu);
        let gpu = gpu.map(|device| {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let renderer = render_gpu::Renderer::new(&device.device, format);
            let frame = device.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frame"),
                size: wgpu::Extent3d {
                    width: u32::from(size.0),
                    height: u32::from(size.1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let canvas = render_gpu::Canvas::new(&device, size.0, size.1, format);
            Gpu {
                device,
                renderer,
                canvas,
                frame,
            }
        });
        Frames {
            gpu,
            cpu: (render_cpu::Renderer::new(), None),
            ppi: 96.0 * scale,
            size,
        }
    }

    fn by(&self) -> String {
        let (w, h) = self.size;
        match &self.gpu {
            Some(gpu) => {
                let info = gpu.device.adapter.get_info();
                format!("render-gpu on {} ({:?}), {w}×{h}", info.name, info.backend)
            }
            None => format!("render-cpu, {w}×{h}"),
        }
    }

    /// Forgets what the canvas shows, as a new window's shows nothing.
    fn forget(&mut self) {
        if let Some(gpu) = &mut self.gpu {
            gpu.canvas.forget();
        }
        self.cpu.1 = None;
    }

    /// Draws `window` as the shell would, chrome and all, and waits for it
    /// to be done. Returns how long making the display list took, and
    /// drawing it.
    fn draw(&mut self, window: &Window) -> anyhow::Result<(f64, f64)> {
        let began = Instant::now();
        let page = window.view();
        let viewed = began.elapsed();
        let (w, h) = self.size;
        match &mut self.gpu {
            Some(gpu) => {
                let rects = gpu.canvas.damage(&page, self.ppi);
                gpu.canvas
                    .draw(
                        &gpu.device,
                        &mut gpu.renderer,
                        page,
                        self.ppi,
                        scene::Color::WHITE,
                        &rects,
                    )
                    .map_err(anyhow::Error::msg)?;
                gpu.canvas.copy_to(&gpu.device, &gpu.frame);
                gpu.device
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())?;
            }
            None => {
                let (renderer, shown) = &mut self.cpu;
                let all = scene::PixelRect {
                    x: 0,
                    y: 0,
                    width: u32::from(w),
                    height: u32::from(h),
                };
                let rects = match shown {
                    Some(old) => scene::redraw(old, &page, self.ppi, u32::from(w), u32::from(h)),
                    None => vec![all],
                };
                for rect in rects {
                    renderer.render_rect(&page, self.ppi, render_cpu::Level::Fastest, rect);
                }
                *shown = Some(page);
            }
        }
        Ok((millis(viewed), millis(began.elapsed() - viewed)))
    }
}
/// Times `event` in `editor` and its frame: the whole, and its parts.
fn frame(
    editor: &mut Window,
    frames: &mut Frames,
    event: Event,
    parts: &mut [Vec<f64>; 3],
) -> anyhow::Result<f64> {
    let began = Instant::now();
    editor.update(event);
    let updated = millis(began.elapsed());
    let (view, draw) = frames.draw(editor)?;
    parts[0].push(updated);
    parts[1].push(view);
    parts[2].push(draw);
    Ok(millis(began.elapsed()))
}

fn named(parts: [Vec<f64>; 3], first: &str) -> Vec<(String, Vec<f64>)> {
    let [a, b, c] = parts;
    vec![
        (first.to_string(), a),
        ("view".to_string(), b),
        ("drawing".to_string(), c),
    ]
}

/// Opening the report, typing in its middle, and scrolling through it.
fn in_editor(
    res: &Arc<Resources>,
    path: &Path,
    frames: &mut Frames,
    runs: usize,
) -> anyhow::Result<Vec<Measured>> {
    let window = || Event::Resize {
        width: WINDOW.0,
        height: WINDOW.1,
    };
    // Opening: the file read, the document laid out, the first frame.
    let mut opening = Vec::new();
    let mut parts: [Vec<f64>; 4] = Default::default();
    for _ in 0..runs {
        // Reading alone, to tell it from laying out.
        let began = Instant::now();
        drop(Store::open(path)?);
        let read = millis(began.elapsed());
        let began = Instant::now();
        let mut editor = Window::open(res.clone(), path)?;
        editor.update(window());
        let opened = millis(began.elapsed());
        // A new window draws all of it.
        frames.forget();
        let (view, draw) = frames.draw(&editor)?;
        opening.push(millis(began.elapsed()));
        parts[0].push(read);
        parts[1].push(opened - read);
        parts[2].push(view);
        parts[3].push(draw);
    }
    let [read, lay_out, view, draw] = parts;
    let open = Measured {
        id: "open",
        samples: opening,
        parts: vec![
            ("read the file".to_string(), read),
            ("lay out".to_string(), lay_out),
            ("view".to_string(), view),
            ("drawing".to_string(), draw),
        ],
        note: "",
    };

    let mut editor = Window::open(res.clone(), path)?;
    editor.update(window());
    // To the middle of the report, a page at a time.
    let paragraphs = editor.editor().document().paragraphs().to_vec();
    let middle = paragraphs.len() / 2;
    let at = |editor: &Window| {
        let caret = editor.editor().selection().1.paragraph;
        paragraphs.iter().position(|&p| p == caret).unwrap_or(0)
    };
    while at(&editor) < middle {
        editor.update(Event::Key {
            key: Key::PageDown,
            modifiers: Modifiers::default(),
        });
    }
    frames.draw(&editor)?;
    let mut typing = Vec::new();
    let mut parts: [Vec<f64>; 3] = Default::default();
    for c in "The quick brown fox jumps over the lazy dog. ".chars() {
        typing.push(frame(
            &mut editor,
            frames,
            Event::Text(c.to_string()),
            &mut parts,
        )?);
    }
    let keystroke = Measured {
        id: "keystroke",
        samples: typing,
        parts: named(parts, "update and save"),
        note: "from the event to the frame done; to the display itself is out of scope",
    };

    // Scrolling a wheel's line at a time (the shell's 48 pixels), down and
    // back up.
    let mut scrolling = Vec::new();
    let mut parts: [Vec<f64>; 3] = Default::default();
    for step in 0..200 {
        let dy = if step < 100 { 48.0 } else { -48.0 };
        scrolling.push(frame(&mut editor, frames, Event::Scroll(dy), &mut parts)?);
    }
    let scroll = Measured {
        id: "scroll",
        samples: scrolling,
        parts: named(parts, "update"),
        note: "every frame at 60 Hz",
    };
    Ok(vec![open, keystroke, scroll])
}

/// Saving after a small edit: appending one transaction to the report's
/// file, and with every hundredth a snapshot of the whole document.
fn saving(path: &Path) -> anyhow::Result<Measured> {
    let opened = Store::open(path)?;
    let (mut store, mut document) = (opened.store, opened.document);
    let paragraph = document.paragraphs()[document.paragraphs().len() / 2];
    let mut times = Vec::new();
    for _ in 0..200 {
        let t = document.transact(
            "Typing",
            vec![Op::InsertText {
                paragraph,
                offset: 0,
                text: "x".to_string(),
                runs: vec![Run::new(1, Props::new())],
            }],
        )?;
        let began = Instant::now();
        store.append(Entry::Commit(&t), &document)?;
        times.push(millis(began.elapsed()));
    }
    Ok(Measured {
        id: "save",
        samples: times,
        parts: Vec::new(),
        note: "the worst is a snapshot of the whole document, every hundredth save",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every budget names a row of ARCHITECTURE.md's table, word for word.
    #[test]
    fn the_budgets_are_the_architectures() {
        let root = testkit::workspace_root();
        let spec: Spec =
            ron::from_str(&std::fs::read_to_string(root.join("spec/budgets.ron")).unwrap())
                .unwrap();
        let architecture = std::fs::read_to_string(root.join("docs/ARCHITECTURE.md")).unwrap();
        let section = architecture
            .split("## 12. Performance budgets")
            .nth(1)
            .and_then(|s| s.split("\n## ").next())
            .expect("section 12");
        let rows: Vec<&str> = section
            .lines()
            .filter_map(|l| l.strip_prefix("| "))
            .filter_map(|l| l.split(" |").next())
            .collect();
        for budget in &spec.budgets {
            assert!(
                rows.contains(&budget.interaction.as_str()),
                "{:?} isn't a row of section 12: {rows:?}",
                budget.interaction
            );
        }
        let ids: std::collections::HashSet<_> = spec.budgets.iter().map(|b| &b.id).collect();
        assert_eq!(ids.len(), spec.budgets.len(), "ids are unique");
    }
}
