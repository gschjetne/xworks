//! The GPU renderer on wgpu, for the shells: display lists drawn by
//! `vello_gpu` (D7, docs/ARCHITECTURE.md, section 10), matching render-cpu,
//! which draws with its twin `vello_cpu`, within a few levels of 255.
//!
//! - [`open`]: a device on the fastest adapter, for a window's surface or
//!   none.
//! - [`Renderer`]: draws a [`scene::Page`] into a texture view, a window's
//!   frame or one read back.
//!
//! render-gpu is an impure crate: it drives the GPU.
//!
//! Invariants:
//!
//! - One renderer draws at any size: the size goes with each frame.
//! - It draws what render-cpu draws, the same items in the same order.

use std::collections::HashMap;
use std::sync::Arc;

use scene::{Color, Item, Page, Path, Segment};
use vello_gpu::kurbo::{Affine, BezPath, Stroke};
use vello_gpu::peniko::{Blob, Fill, FontData};

/// An open GPU.
pub struct Device {
    /// The adapter it's on.
    pub adapter: wgpu::Adapter,
    /// The device.
    pub device: wgpu::Device,
    /// Its queue.
    pub queue: wgpu::Queue,
}

/// A device on the fastest adapter `instance` has that can draw on
/// `surface`, if given; `None` if there's none. `WGPU_BACKEND` (vulkan,
/// dx12, metal, gl) picks a backend.
pub fn open(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Option<Device> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: surface,
        ..Default::default()
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    Some(Device {
        adapter,
        device,
        queue,
    })
}

/// Draws display lists into textures of one format.
pub struct Renderer {
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    fonts: HashMap<(usize, u32), FontData>,
}

/// The size in pixels of a `units`-long side at `ppi` pixels per inch,
/// rounded up, between 1 and 65 535: as render-cpu sizes its images.
pub fn pixels(units: i64, ppi: f64) -> u16 {
    let n = (units as f64 * ppi / 914_400.0).ceil();
    n.clamp(1.0, f64::from(u16::MAX)) as u16
}

impl Renderer {
    /// A renderer for textures of `format`, compiling its shaders.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Renderer {
        let (renderer, resources) = vello_gpu::Renderer::new(
            device,
            &vello_gpu::RenderTargetConfig {
                format,
                width: 1,
                height: 1,
            },
        );
        Renderer {
            renderer,
            resources,
            fonts: HashMap::new(),
        }
    }

    /// Draws `page` at `ppi` into `view`, `width` by `height` pixels, over
    /// `background`, and submits the work.
    #[allow(
        clippy::too_many_arguments,
        reason = "a frame needs its device, page, size and target"
    )]
    pub fn render(
        &mut self,
        device: &Device,
        page: &Page,
        ppi: f64,
        view: &wgpu::TextureView,
        width: u16,
        height: u16,
        background: Color,
    ) -> Result<(), String> {
        let mut scene = vello_gpu::Scene::new(width, height);
        let scale = ppi / 914_400.0;
        let at = |v: i64| v as f64 * scale;
        scene.set_fill_rule(Fill::NonZero);
        for item in &page.items {
            match item {
                Item::Fill { path, color } => {
                    scene.set_paint(paint(*color));
                    scene.fill_path(&bez(path, scale));
                }
                Item::Stroke { path, width, color } => {
                    scene.set_paint(paint(*color));
                    scene.set_stroke(Stroke::new(at(*width)));
                    scene.stroke_path(&bez(path, scale));
                }
                Item::Glyphs(run) => {
                    let font = self
                        .fonts
                        .entry(run.font.key())
                        .or_insert_with(|| {
                            FontData::new(Blob::new(Arc::clone(&run.font.data)), run.font.index)
                        })
                        .clone();
                    scene.set_paint(paint(run.color));
                    let mut builder = scene
                        .glyph_run(&mut self.resources, &font)
                        .font_size(at(run.size) as f32)
                        .hint(false);
                    if run.oblique {
                        builder = builder.glyph_transform(Affine::skew(-scene::OBLIQUE_SLANT, 0.0));
                    }
                    let _ = builder.fill_glyphs(run.glyphs.iter().map(|g| glifo::Glyph {
                        id: g.id,
                        x: at(g.at.x) as f32,
                        y: at(g.at.y) as f32,
                    }));
                }
            }
        }
        let mut encoder = device.device.create_command_encoder(&Default::default());
        self.renderer
            .render(
                &scene,
                &mut self.resources,
                &device.device,
                &device.queue,
                &mut encoder,
                &vello_gpu::RenderSize { width, height },
                view,
                None,
                &vello_gpu::TextureBindings::new(),
                vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::Viewport {
                    color: paint(background),
                }),
            )
            .map_err(|e| format!("{e:?}"))?;
        device.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// `page` drawn at `ppi` into a texture and read back: RGBA, 8 bits a
    /// channel, premultiplied by its alpha, row by row. For tests.
    pub fn render_offscreen(
        &mut self,
        device: &Device,
        page: &Page,
        ppi: f64,
    ) -> Result<(u16, u16, Vec<u8>), String> {
        let (w, h) = (pixels(page.width, ppi), pixels(page.height, ppi));
        let texture = device.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: u32::from(w),
                height: u32::from(h),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render(
            device,
            page,
            ppi,
            &view,
            w,
            h,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        )?;
        Ok((w, h, read(device, &texture, w, h)))
    }
}

/// `texture`'s pixels, row by row.
fn read(device: &Device, texture: &wgpu::Texture, width: u16, height: u16) -> Vec<u8> {
    let (w, h) = (u32::from(width), u32::from(height));
    let row =
        (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    device.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("the readback maps"));
    let _ = device.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("the readback is mapped");
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let start = (y * row) as usize;
        pixels.extend_from_slice(&data[start..start + (w * 4) as usize]);
    }
    pixels
}

fn paint(c: Color) -> vello_gpu::color::AlphaColor<vello_gpu::color::Srgb> {
    vello_gpu::color::AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn bez(path: &Path, scale: f64) -> BezPath {
    let p = |q: scene::Point| (q.x as f64 * scale, q.y as f64 * scale);
    let mut out = BezPath::new();
    for segment in &path.0 {
        match *segment {
            Segment::MoveTo(a) => out.move_to(p(a)),
            Segment::LineTo(a) => out.line_to(p(a)),
            Segment::QuadTo(a, b) => out.quad_to(p(a), p(b)),
            Segment::CubicTo(a, b, c) => out.curve_to(p(a), p(b), p(c)),
            Segment::Close => out.close_path(),
        }
    }
    out
}
