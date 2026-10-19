//! Vello's sparse-strips renderers: `vello_cpu`, the reference, and
//! `vello_gpu`, which shares its architecture and its drawing API.

use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::Stroke;
use vello_cpu::peniko::{Blob, FontData, Gradient};

use crate::corpus::{Fonts, Item, Page, Paint, Rgba};

fn color(c: Rgba) -> vello_cpu::peniko::Color {
    AlphaColor::from_rgba8(c[0], c[1], c[2], c[3])
}

pub fn gradient(paint: &Paint) -> Option<Gradient> {
    let stops = |stops: &[(f32, Rgba)]| -> Vec<(f32, vello_cpu::peniko::Color)> {
        stops.iter().map(|&(t, c)| (t, color(c))).collect()
    };
    match paint {
        Paint::Solid(_) => None,
        Paint::Linear { start, end, stops: s } => {
            Some(Gradient::new_linear(*start, *end).with_stops(stops(s).as_slice()))
        }
        Paint::Radial { center, radius, stops: s } => {
            Some(Gradient::new_radial(*center, *radius).with_stops(stops(s).as_slice()))
        }
    }
}

/// The fonts as Vello's font handles.
pub fn font_data(fonts: &Fonts) -> Vec<FontData> {
    fonts
        .data
        .iter()
        .map(|d| FontData::new(Blob::new(d.clone()), 0))
        .collect()
}

/// Draws `page` on a vello_cpu `RenderContext` or a vello_gpu `Scene`,
/// whose drawing methods are the same; `$cache` turns on the experimental
/// glyph atlas cache.
macro_rules! draw_page {
    ($ctx:expr, $resources:expr, $page:expr, $fonts:expr, $cache:expr) => {{
        for item in &$page.items {
            match item {
                Item::Fill { path, paint } => {
                    set_paint!($ctx, paint);
                    $ctx.fill_path(path);
                }
                Item::Stroke { path, width, paint } => {
                    set_paint!($ctx, paint);
                    $ctx.set_stroke(Stroke::new(*width));
                    $ctx.stroke_path(path);
                }
                Item::Glyphs { run } => {
                    $ctx.set_paint(color(run.color));
                    $ctx.glyph_run($resources, &$fonts[run.font])
                        .font_size(run.size)
                        .hint(false)
                        .atlas_cache($cache)
                        .fill_glyphs(run.glyphs.iter().map(|&(id, x, y)| {
                            vello_cpu::Glyph { id, x, y }
                        }))
                        .expect("glyphs render");
                }
            }
        }
    }};
}

macro_rules! set_paint {
    ($ctx:expr, $paint:expr) => {
        match $paint {
            Paint::Solid(c) => $ctx.set_paint(color(*c)),
            other => $ctx.set_paint(gradient(other).expect("a gradient")),
        }
    };
}

/// Renders `page` with vello_cpu at SIMD level `level`, on `threads`
/// worker threads (0: the calling thread). Returns RGBA8 pixels,
/// premultiplied.
pub fn render_cpu(
    page: &Page,
    fonts: &[FontData],
    level: vello_cpu::Level,
    threads: u16,
) -> Vec<u8> {
    let mut cpu = Cpu::new(page, level, threads);
    cpu.render(page, fonts, false);
    cpu.pixels()
}

/// vello_cpu kept across frames, as an app keeps it, so caches persist.
pub struct Cpu {
    ctx: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    pixmap: vello_cpu::Pixmap,
}

impl Cpu {
    pub fn new(page: &Page, level: vello_cpu::Level, threads: u16) -> Cpu {
        let settings = vello_cpu::RenderSettings {
            level,
            num_threads: threads,
        };
        Cpu {
            ctx: vello_cpu::RenderContext::new_with(page.width, page.height, settings),
            resources: vello_cpu::Resources::new(),
            pixmap: vello_cpu::Pixmap::new(page.width, page.height),
        }
    }

    /// Renders a frame of `page`, with or without the glyph cache.
    pub fn render(&mut self, page: &Page, fonts: &[FontData], cache: bool) {
        self.ctx.reset();
        draw_page!(self.ctx, &mut self.resources, page, fonts, cache);
        self.ctx.flush();
        self.ctx.render(&mut self.pixmap, &mut self.resources);
    }

    /// The last frame: RGBA8 pixels, premultiplied.
    pub fn pixels(&self) -> Vec<u8> {
        self.pixmap
            .data()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect()
    }
}

/// vello_gpu, ready to render pages of one size.
pub struct Gpu {
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
}

impl Gpu {
    /// Creates the renderer, compiling its shaders.
    pub fn new(device: &wgpu::Device, width: u16, height: u16) -> Gpu {
        let (renderer, resources) = vello_gpu::Renderer::new(
            device,
            &vello_gpu::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width,
                height,
            },
        );
        Gpu {
            renderer,
            resources,
        }
    }

    /// Records `page` into `encoder`, rendering it into `view`, with or
    /// without the glyph cache.
    pub fn render(
        &mut self,
        page: &Page,
        fonts: &[FontData],
        cache: bool,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let mut scene = vello_gpu::Scene::new(page.width, page.height);
        draw_page!(scene, &mut self.resources, page, fonts, cache);
        self.renderer
            .render(
                &scene,
                &mut self.resources,
                device,
                queue,
                encoder,
                &vello_gpu::RenderSize {
                    width: page.width,
                    height: page.height,
                },
                view,
                None,
                &vello_gpu::TextureBindings::new(),
                vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::Viewport {
                    color: color([255, 255, 255, 255]),
                }),
            )
            .expect("vello_gpu renders");
    }
}
