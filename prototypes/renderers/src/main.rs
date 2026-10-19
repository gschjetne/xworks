//! The renderer comparison (docs/ROADMAP.md, M1 stage 3, decision D7).
//!
//! ```text
//! ./fetch-fonts.sh                          the corpus's fonts, once
//! cargo run --release -- adapters           the GPUs wgpu finds
//! cargo run --release -- render             every candidate's renders, to target/out/
//! cargo run --release -- hashes             CPU renders' hashes, per SIMD level
//! cargo run --release -- bench [FRAMES]     frame times, CPU and GPU
//! cargo run --release -- diff               how far each render is from vello_cpu's
//! ```
//!
//! `--fonts DIR` reads the fonts from elsewhere; `WGPU_BACKEND` picks the
//! GPU backend. GPU commands skip what has no adapter.

mod corpus;
mod gpu;
#[cfg(feature = "research")]
mod research;
#[cfg(feature = "skia")]
mod skia;
#[cfg(feature = "sparse")]
mod sparse;

use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::{Fonts, Page};
use sha2::{Digest, Sha256};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut fonts_dir = PathBuf::from("target/fonts");
    if let Some(i) = args.iter().position(|a| a == "--fonts") {
        fonts_dir = PathBuf::from(args.remove(i + 1));
        args.remove(i);
    }
    let command = args.first().cloned().unwrap_or_default();
    if command == "adapters" {
        adapters();
        return;
    }
    let fonts = Fonts::load(&fonts_dir).unwrap_or_else(|e| {
        eprintln!("{}: {e}; run ./fetch-fonts.sh", fonts_dir.display());
        std::process::exit(2);
    });
    let pages = corpus::corpus(&fonts);
    match command.as_str() {
        "render" => render(&pages, &fonts),
        "hashes" => hashes(&pages, &fonts),
        "bench" => {
            let frames = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20);
            bench(&pages, &fonts, frames);
        }
        #[cfg(feature = "sparse")]
        "diff" => diff(&pages, &fonts),
        _ => eprintln!("usage: renderer-prototype [--fonts DIR] adapters | render | hashes | bench [FRAMES] | diff"),
    }
}

fn adapters() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    for adapter in pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all())) {
        let i = adapter.get_info();
        println!("{:?} {:?} {} | {} {}", i.backend, i.device_type, i.name, i.driver, i.driver_info);
    }
}

fn hash(pixels: &[u8]) -> String {
    Sha256::digest(pixels)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn write_png(path: &Path, page: &Page, pixels: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, page.width as u32, page.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

/// The median of `runs` timings of `f`, in milliseconds.
fn time_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    times[times.len() / 2]
}

/// The SIMD levels vello_cpu can render at on this computer: the one it
/// detects, the platform's baseline, and the scalar fallback when built
/// with the `fallback` feature.
#[cfg(feature = "sparse")]
fn levels() -> Vec<(String, vello_cpu::Level)> {
    let detected = vello_cpu::Level::try_detect().unwrap_or(vello_cpu::Level::baseline());
    let name = |l: vello_cpu::Level| format!("{l:?}").split('(').next().unwrap_or("?").to_string();
    let mut levels = vec![(name(detected), detected)];
    let baseline = vello_cpu::Level::baseline();
    if name(baseline) != name(detected) {
        levels.push((name(baseline), baseline));
    }
    #[cfg(feature = "fallback")]
    levels.push(("Fallback".into(), vello_cpu::Level::fallback()));
    levels
}

/// Every candidate's render of every page, as PNGs in target/out/, and per
/// page a sheet of the same region from each, magnified four times, top to
/// bottom in the order the renders are listed.
fn render(pages: &[Page], fonts: &Fonts) {
    let out = Path::new("target/out");
    let mut renders: Vec<(&str, &Page, Vec<u8>)> = Vec::new();
    for page in pages {
        #[cfg(feature = "sparse")]
        {
            let data = sparse::font_data(fonts);
            renders.push(("vello_cpu", page, sparse::render_cpu(page, &data, levels()[0].1, 0)));
        }
        #[cfg(feature = "skia")]
        renders.push(("tiny-skia", page, skia::render(page, fonts)));
    }
    renders.extend(gpu_renders(pages, fonts));
    for (name, page, pixels) in &renders {
        write_png(&out.join(format!("{}-{name}.png", page.name)), page, pixels);
    }
    // A region of small text on each page: (x, y, width, height).
    let regions = [("text", (189, 330, 300, 90)), ("grid", (40, 0, 300, 90)), ("slide", (170, 320, 300, 90))];
    for (name, (x, y, w, h)) in regions {
        let crops: Vec<_> = renders.iter().filter(|r| r.1.name == name).collect();
        let scale = 4;
        let (sw, sh) = (w * scale, (h * scale + 8) * crops.len());
        let mut sheet = vec![255u8; sw * sh * 4];
        for (i, (_, page, pixels)) in crops.iter().enumerate() {
            for sy in 0..h * scale {
                for sx in 0..sw {
                    let src = ((y + sy / scale) * page.width as usize + x + sx / scale) * 4;
                    let dst = ((i * (h * scale + 8) + sy) * sw + sx) * 4;
                    sheet[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
                }
            }
        }
        let sheet_page = Page { name, width: sw as u16, height: sh as u16, items: Vec::new() };
        let order: Vec<_> = crops.iter().map(|c| c.0).collect();
        write_png(&out.join(format!("{name}-crops.png")), &sheet_page, &sheet);
        println!("{name}-crops.png: {}", order.join(", "));
    }
    println!("{}", out.display());
}

/// The GPU candidates' renders, read back: (candidate, page, pixels).
fn gpu_renders<'p>(pages: &'p [Page], fonts: &Fonts) -> Vec<(&'static str, &'p Page, Vec<u8>)> {
    let mut renders = Vec::new();
    let Some(gpu) = gpu::open() else {
        eprintln!("no GPU adapter; GPU renders skipped");
        return renders;
    };
    for page in pages {
        let (texture, view) = gpu.target(page.width, page.height);
        #[cfg(feature = "sparse")]
        {
            let data = sparse::font_data(fonts);
            let mut renderer = sparse::Gpu::new(&gpu.device, page.width, page.height);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(page, &data, false, &gpu.device, &gpu.queue, &mut encoder, &view);
            gpu.queue.submit([encoder.finish()]);
            renders.push(("vello_gpu", page, gpu.read(&texture, page.width, page.height)));
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            renderer.render(page, &data, true, &gpu.device, &gpu.queue, &mut encoder, &view);
            gpu.queue.submit([encoder.finish()]);
            renders.push(("vello_gpu, glyph cache", page, gpu.read(&texture, page.width, page.height)));
        }
        #[cfg(feature = "research")]
        {
            let data = research::font_data(fonts);
            let mut renderer = research::renderer(&gpu.device);
            let scene = research::scene(page, &data);
            research::render(&mut renderer, page, &scene, &gpu.device, &gpu.queue, &view);
            renders.push(("vello", page, gpu.read(&texture, page.width, page.height)));
        }
    }
    renders
}

/// Hashes of the CPU renders, per SIMD level: identical hashes across levels
/// and platforms mean the same pixels everywhere.
fn hashes(pages: &[Page], fonts: &Fonts) {
    println!("| Page | Renderer | SIMD level | SHA-256 (first 8 bytes) |");
    println!("|---|---|---|---|");
    for page in pages {
        #[cfg(feature = "sparse")]
        {
            let data = sparse::font_data(fonts);
            for (name, level) in levels() {
                let pixels = sparse::render_cpu(page, &data, level, 0);
                println!("| {} | vello_cpu | {name} | {} |", page.name, hash(&pixels));
            }
            let threaded = sparse::render_cpu(page, &data, levels()[0].1, 4);
            println!("| {} | vello_cpu, 4 threads | {} | {} |", page.name, levels()[0].0, hash(&threaded));
        }
        #[cfg(feature = "skia")]
        println!("| {} | tiny-skia | (its own) | {} |", page.name, hash(&skia::render(page, fonts)));
    }
}

/// How far `b` is from `a`: the largest difference in any channel, the share
/// of pixels differing by more than 1 and more than 16, and the mean
/// absolute difference per channel.
fn difference(a: &[u8], b: &[u8]) -> String {
    let (mut max, mut over1, mut over16, mut sum) = (0u8, 0usize, 0usize, 0u64);
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d = pa.iter().zip(pb).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        sum += pa.iter().zip(pb).map(|(x, y)| x.abs_diff(*y) as u64).sum::<u64>();
        max = max.max(d);
        over1 += (d > 1) as usize;
        over16 += (d > 16) as usize;
    }
    let n = a.len() / 4;
    format!(
        "{max} | {:.2}% | {:.3}% | {:.3}",
        over1 as f64 * 100.0 / n as f64,
        over16 as f64 * 100.0 / n as f64,
        sum as f64 / a.len() as f64
    )
}

/// Each candidate's render compared with vello_cpu's, the reference.
#[cfg(feature = "sparse")]
fn diff(pages: &[Page], fonts: &Fonts) {
    let data = sparse::font_data(fonts);
    let reference: Vec<Vec<u8>> = pages
        .iter()
        .map(|p| sparse::render_cpu(p, &data, levels()[0].1, 0))
        .collect();
    println!("| Page | Candidate | largest difference | pixels > 1 | pixels > 16 | mean |");
    println!("|---|---|---|---|---|---|");
    for (page, reference) in pages.iter().zip(&reference) {
        for (name, level) in levels().into_iter().skip(1) {
            let other = sparse::render_cpu(page, &data, level, 0);
            println!("| {} | vello_cpu at {name} | {} |", page.name, difference(reference, &other));
        }
        let mut cached = sparse::Cpu::new(page, levels()[0].1, 0);
        cached.render(page, &data, true);
        cached.render(page, &data, true);
        println!("| {} | vello_cpu, glyph cache | {} |", page.name, difference(reference, &cached.pixels()));
        #[cfg(feature = "skia")]
        println!("| {} | tiny-skia | {} |", page.name, difference(reference, &skia::render(page, fonts)));
    }
    // The cached GPU render against the cached CPU render: how they'd
    // compare if both used the glyph cache.
    let cached: Vec<Vec<u8>> = pages
        .iter()
        .map(|p| {
            let mut cpu = sparse::Cpu::new(p, levels()[0].1, 0);
            cpu.render(p, &data, true);
            cpu.pixels()
        })
        .collect();
    for (name, page, pixels) in gpu_renders(pages, fonts) {
        let i = pages.iter().position(|p| std::ptr::eq(p, page)).unwrap();
        println!("| {} | {name} | {} |", page.name, difference(&reference[i], &pixels));
        if name.ends_with("glyph cache") {
            println!(
                "| {} | {name}, against vello_cpu's glyph cache | {} |",
                page.name,
                difference(&cached[i], &pixels)
            );
        }
    }
}

/// Frame times: each CPU candidate on one thread (and vello_cpu on more),
/// and each GPU candidate from building its scene to the GPU finishing.
/// Also the GPU renderers' start-up: creating them, shaders and all, and
/// their first frame.
fn bench(pages: &[Page], fonts: &Fonts, frames: usize) {
    println!("Frames: median of {frames}.\n");
    println!("| Page | glyphs | Candidate | frame (ms) |");
    println!("|---|---|---|---|");
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8) as u16;
    for page in pages {
        let g = page.glyph_count();
        #[cfg(feature = "sparse")]
        {
            let data = sparse::font_data(fonts);
            let level = levels()[0].1;
            let t = time_ms(frames, || {
                std::hint::black_box(sparse::render_cpu(page, &data, level, 0));
            });
            println!("| {} | {g} | vello_cpu, 1 thread | {t:.2} |", page.name);
            let t = time_ms(frames, || {
                std::hint::black_box(sparse::render_cpu(page, &data, level, threads));
            });
            println!("| {} | {g} | vello_cpu, {threads} threads | {t:.2} |", page.name);
            let mut cpu = sparse::Cpu::new(page, level, 0);
            let t = time_ms(frames, || cpu.render(page, &data, true));
            println!("| {} | {g} | vello_cpu, 1 thread, glyph cache | {t:.2} |", page.name);
            let mut cpu = sparse::Cpu::new(page, level, threads);
            let t = time_ms(frames, || cpu.render(page, &data, true));
            println!("| {} | {g} | vello_cpu, {threads} threads, glyph cache | {t:.2} |", page.name);
        }
        #[cfg(feature = "skia")]
        {
            let t = time_ms(frames, || {
                std::hint::black_box(skia::render(page, fonts));
            });
            println!("| {} | {g} | tiny-skia, 1 thread | {t:.2} |", page.name);
        }
    }

    let Some(gpu) = gpu::open() else {
        eprintln!("\nno GPU adapter; GPU timings skipped");
        return;
    };
    println!(
        "\nGPU: {} ({:?}, {})\n",
        gpu.info.name, gpu.info.backend, gpu.info.driver_info
    );
    println!("| Page | Candidate | start-up (ms) | first frame (ms) | frame (ms) |");
    println!("|---|---|---|---|---|");
    for page in pages {
        let (texture, view) = gpu.target(page.width, page.height);
        #[cfg(feature = "sparse")]
        {
            let data = sparse::font_data(fonts);
            let start = Instant::now();
            let mut renderer = sparse::Gpu::new(&gpu.device, page.width, page.height);
            gpu.wait();
            let startup = start.elapsed().as_secs_f64() * 1000.0;
            for cache in [false, true] {
                let mut frame = || {
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    renderer.render(page, &data, cache, &gpu.device, &gpu.queue, &mut encoder, &view);
                    gpu.queue.submit([encoder.finish()]);
                    gpu.wait();
                };
                let first = time_ms(1, &mut frame);
                let t = time_ms(frames, &mut frame);
                let name = if cache { "vello_gpu, glyph cache" } else { "vello_gpu" };
                println!("| {} | {name} | {startup:.0} | {first:.1} | {t:.2} |", page.name);
            }
        }
        #[cfg(feature = "research")]
        {
            let data = research::font_data(fonts);
            let start = Instant::now();
            let mut renderer = research::renderer(&gpu.device);
            gpu.wait();
            let startup = start.elapsed().as_secs_f64() * 1000.0;
            let mut frame = || {
                let scene = research::scene(page, &data);
                research::render(&mut renderer, page, &scene, &gpu.device, &gpu.queue, &view);
                gpu.wait();
            };
            let first = time_ms(1, &mut frame);
            let t = time_ms(frames, &mut frame);
            println!("| {} | vello | {startup:.0} | {first:.1} | {t:.2} |", page.name);
        }
        #[cfg(feature = "skia")]
        {
            let t = time_ms(frames, || {
                let pixels = skia::render(page, fonts);
                gpu.upload(&texture, page.width, page.height, &pixels);
                gpu.wait();
            });
            println!("| {} | tiny-skia, uploaded | – | – | {t:.2} |", page.name);
        }
        drop(texture);
    }
}
