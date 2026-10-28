//! The GPU renderer on wgpu, for the shells: display lists drawn by
//! `vello_gpu` (D7, docs/ARCHITECTURE.md, section 10), matching render-cpu,
//! which draws with its twin `vello_cpu`, so goldens rendered on the CPU
//! hold for the screen.
//!
//! - [`open`]: a device on the fastest adapter, for a window's surface or
//!   none.
//! - [`Renderer`]: draws a [`scene::Page`] into a texture view, a window's
//!   frame or one read back, all of it or only some rectangles of it.
//! - [`Canvas`]: a window's own picture, kept between frames: each frame
//!   draws into it only where the display list changed, by the GPU or by
//!   writing render-cpu's pixels, and it's copied whole to the window, as
//!   wgpu presents only whole frames.
//!
//! render-gpu is an impure crate: it drives the GPU.
//!
//! # What a caller may rely on
//!
//! Its tests need a GPU, so CI doesn't run them: they're run by hand, on
//! Windows from WSL (CLAUDE.md), `tests/matches_cpu.rs` with
//! `--run-ignored only`.
//!
//! - **It draws what render-cpu draws:** the same items in the same order,
//!   images from the same level, within 8 levels of 255 on text, fills,
//!   strokes, images, clips and transforms (`tests/matches_cpu.rs`,
//!   `the_gpu_draws_what_the_cpu_draws` and
//!   `images_clips_and_transforms_draw_as_on_the_cpu`).
//! - **One renderer draws at any size:** the size goes with each frame.
//! - **A canvas shows the page last drawn into it:** drawing only where
//!   [`Canvas::damage`] says gives what drawing it all would, within a
//!   level, and a canvas drawing nothing yet, or at another resolution,
//!   draws everything (`tests/matches_cpu.rs`,
//!   `a_canvas_drawn_where_it_changed_shows_the_new_page`); render-cpu's
//!   pixels written into it are kept as they are, in either of the
//!   formats a window's frame has, cut at its edges
//!   (`render_cpus_pixels_are_written_as_they_are`).
//! - **The atlas never runs out of room:** images live in vello_gpu's
//!   atlas only while they're drawn, with a texel of border copied from
//!   their edges, at most [`MOST_IMAGES`] at once, each fitting a page of
//!   it, and there are as many pages as images; a page drawing more
//!   shows the rest as grey (`tests/matches_cpu.rs`,
//!   `more_images_than_the_atlas_holds_draw_grey`).
//! - **Errors, not panics,** where the GPU fails a frame: [`Renderer::render`]
//!   and [`Canvas::draw`] say why, and [`open`] is `None` where there's no
//!   adapter.
//!
//! # What it doesn't promise
//!
//! - render-cpu's pixels exactly: the two differ by a few levels, more on
//!   gradients, and goldens are rendered on the CPU.
//! - That it runs: a machine may have no GPU, or only a software one, as
//!   WSL's llvmpipe is, where the shells draw with render-cpu instead.
//! - That its errors are typed: they're messages, for the log.
//! - An image put in the atlas in the frame another leaves it shows as
//!   nothing: vello_gpu clears the space the one leaving had in the
//!   frame's passes, after render-gpu has written the new one there
//!   (`tests/matches_cpu.rs`, `an_image_put_where_another_just_left_shows`,
//!   ignored as failing).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use scene::{Color, Item, Page, Path, PixelRect, Segment};
use vello_gpu::kurbo::{self, Affine, BezPath, Stroke};
use vello_gpu::peniko::{Blob, Extend, Fill, FontData, ImageQuality, ImageSampler};
use vello_gpu::{AtlasConfig, ImageId, ImageSource, RectU16};

/// The most images the atlas holds at once. A page drawing more than this
/// many shows the rest as grey.
pub const MOST_IMAGES: usize = 64;

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
    /// The image levels in the atlas, by image and level.
    images: HashMap<(usize, usize), Uploaded>,
    /// How many frames it has drawn, for forgetting images.
    frames: u64,
}

/// An image level in the atlas.
struct Uploaded {
    /// The image, kept so its key can't come to mean another.
    _image: scene::Image,
    id: ImageId,
    used: u64,
}

/// Frames after which an image not drawn in them leaves the atlas.
const FORGET_AFTER: u64 = 32;

/// The side of an atlas page: the largest image, and its border.
const ATLAS_SIDE: u16 = scene::MAX_SIDE as u16 + 2;

/// The size in pixels of a `units`-long side at `ppi` pixels per inch,
/// rounded up, between 1 and 65 535: as render-cpu sizes its images.
pub fn pixels(units: i64, ppi: f64) -> u16 {
    let n = (units as f64 * ppi / 914_400.0).ceil();
    n.clamp(1.0, f64::from(u16::MAX)) as u16
}

impl Renderer {
    /// A renderer for textures of `format`, compiling its shaders.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Renderer {
        let settings = vello_gpu::RenderSettings {
            memory_settings: vello_gpu::MemorySettings {
                image_atlas_config: AtlasConfig {
                    max_atlases: MOST_IMAGES,
                    atlas_size: (ATLAS_SIDE, ATLAS_SIDE),
                    ..AtlasConfig::default()
                },
                ..vello_gpu::MemorySettings::default()
            },
            ..vello_gpu::RenderSettings::default()
        };
        let (renderer, resources) = vello_gpu::Renderer::new_with(
            device,
            &vello_gpu::RenderTargetConfig {
                format,
                width: 1,
                height: 1,
            },
            settings,
        );
        Renderer {
            renderer,
            resources,
            fonts: HashMap::new(),
            images: HashMap::new(),
            frames: 0,
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
        self.draw(device, page, ppi, view, (width, height), background, None)
    }

    /// Draws `rects` of `page` at `ppi` into `view`, `width` by `height`
    /// pixels, over `background`, leaving the rest of it as it was, and
    /// submits the work. Only the items reaching into them are drawn.
    #[allow(
        clippy::too_many_arguments,
        reason = "a frame needs its device, page, size and target"
    )]
    pub fn render_rects(
        &mut self,
        device: &Device,
        page: &Page,
        ppi: f64,
        view: &wgpu::TextureView,
        (width, height): (u16, u16),
        background: Color,
        rects: &[PixelRect],
    ) -> Result<(), String> {
        if rects.is_empty() {
            return Ok(());
        }
        self.draw(
            device,
            page,
            ppi,
            view,
            (width, height),
            background,
            Some(rects),
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a frame needs its device, page, size and target"
    )]
    fn draw(
        &mut self,
        device: &Device,
        page: &Page,
        ppi: f64,
        view: &wgpu::TextureView,
        (width, height): (u16, u16),
        background: Color,
        rects: Option<&[PixelRect]>,
    ) -> Result<(), String> {
        self.frames += 1;
        let scale = ppi / 914_400.0;
        // The rectangles in layout units, a couple of pixels larger, to
        // leave out the items that don't reach them.
        let areas: Option<Vec<scene::Rect>> = rects.map(|rects| {
            let units = |v: u32| (f64::from(v) / scale).floor() as i64;
            let pixel = (1.0 / scale).ceil() as i64;
            rects
                .iter()
                .map(|r| {
                    scene::Rect::new(units(r.x), units(r.y), units(r.width), units(r.height))
                        .outset(2 * pixel)
                })
                .collect()
        });
        let drawn: Vec<&Item> = page
            .items
            .iter()
            .filter(|item| match &areas {
                None => true,
                Some(areas) => item
                    .bounds()
                    .is_some_and(|b| areas.iter().any(|a| b.intersection(*a).is_some())),
            })
            .collect();
        let mut encoder = device.device.create_command_encoder(&Default::default());
        let ids = self.upload(device, &mut encoder, &drawn, scale);
        let mut scene = vello_gpu::Scene::new(width, height);
        scene.set_fill_rule(Fill::NonZero);
        if let Some(rects) = rects {
            let mut clip = BezPath::new();
            for r in rects {
                let (x0, y0) = (f64::from(r.x), f64::from(r.y));
                let (x1, y1) = (x0 + f64::from(r.width), y0 + f64::from(r.height));
                clip.move_to((x0, y0));
                clip.line_to((x1, y0));
                clip.line_to((x1, y1));
                clip.line_to((x0, y1));
                clip.close_path();
            }
            scene.push_clip_path(&clip);
        }
        let mut drawer = Drawer {
            scene: &mut scene,
            resources: &mut self.resources,
            fonts: &mut self.fonts,
            ids: &ids,
            scale,
        };
        for item in drawn {
            drawer.item(item, None);
        }
        if rects.is_some() {
            scene.pop_clip();
        }
        let clears: Vec<RectU16> = rects
            .unwrap_or_default()
            .iter()
            .map(|r| {
                let at = |v: u32| v.min(u32::from(u16::MAX)) as u16;
                RectU16::new(at(r.x), at(r.y), at(r.x + r.width), at(r.y + r.height))
            })
            .collect();
        let clear = match rects {
            None => vello_gpu::ClearSettings::Viewport {
                color: paint(background),
            },
            Some(_) => vello_gpu::ClearSettings::Rects {
                color: paint(background),
                rects: &clears,
            },
        };
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
                vello_gpu::TargetInit::Clear(clear),
            )
            .map_err(|e| format!("{e:?}"))?;
        device.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Puts the image levels `items` draw in the atlas, if they aren't
    /// there, making room by taking out those not drawn now, longest
    /// unused first; and takes out those unused for a while. Returns where
    /// each is, by image and level.
    fn upload(
        &mut self,
        device: &Device,
        encoder: &mut wgpu::CommandEncoder,
        items: &[&Item],
        scale: f64,
    ) -> HashMap<(usize, usize), (ImageId, bool)> {
        let mut wanted: Vec<(scene::Image, usize)> = Vec::new();
        for item in items {
            levels(item, scale, Affine::IDENTITY, &mut wanted);
        }
        let frames = self.frames;
        let keys: Vec<(usize, usize)> = wanted.iter().map(|(i, l)| (i.key(), *l)).collect();
        let mut stale: Vec<((usize, usize), u64)> = self
            .images
            .iter()
            .filter(|(key, _)| !keys.contains(key))
            .map(|(key, u)| (*key, u.used))
            .collect();
        stale.sort_by_key(|(_, used)| *used);
        let missing = keys.iter().filter(|k| !self.images.contains_key(k)).count();
        for (key, used) in stale {
            let crowded = self.images.len() + missing > MOST_IMAGES;
            if (crowded || frames - used > FORGET_AFTER)
                && let Some(gone) = self.images.remove(&key)
            {
                self.renderer
                    .destroy_image(&mut self.resources, encoder, gone.id);
            }
        }
        let mut ids = HashMap::new();
        for (image, level) in wanted {
            let key = (image.key(), level);
            if !self.images.contains_key(&key) {
                if self.images.len() >= MOST_IMAGES {
                    continue;
                }
                let id = self.renderer.upload_image(
                    &mut self.resources,
                    &device.device,
                    &device.queue,
                    encoder,
                    &Bordered::new(&image.levels()[level]),
                );
                self.images.insert(
                    key,
                    Uploaded {
                        _image: image.clone(),
                        id,
                        used: frames,
                    },
                );
            }
            if let Some(uploaded) = self.images.get_mut(&key) {
                uploaded.used = frames;
                ids.insert(key, (uploaded.id, image.opaque()));
            }
        }
        ids
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

/// The image levels `item` draws, through `transform` from its
/// coordinates in pixels to the canvas's, added to `out` if they aren't
/// in it.
fn levels(item: &Item, scale: f64, transform: Affine, out: &mut Vec<(scene::Image, usize)>) {
    match item {
        Item::Image { image, rect, .. } => {
            let level = level_of(image, rect, scale, transform);
            if !out.iter().any(|(i, l)| i.same(image) && *l == level) {
                out.push((image.clone(), level));
            }
        }
        Item::Group(group) => {
            let inner = transform * group_affine(&group.transform, scale);
            for item in &group.items {
                levels(item, scale, inner, out);
            }
        }
        _ => {}
    }
}

/// Which level of `image` to draw it from in `rect`, through `transform`:
/// as render-cpu chooses.
fn level_of(image: &scene::Image, rect: &scene::Rect, scale: f64, transform: Affine) -> usize {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let (w, h) = (rect.width as f64 * scale, rect.height as f64 * scale);
    image.level_for(w * a.hypot(b), h * c.hypot(d))
}

/// A group's transform, for coordinates in pixels.
fn group_affine(transform: &scene::Transform, scale: f64) -> Affine {
    let [a, b, c, d, e, f] = transform.0;
    Affine::new([a, b, c, d, e * scale, f * scale])
}

/// What drawing a page's items needs.
struct Drawer<'a> {
    scene: &'a mut vello_gpu::Scene,
    resources: &'a mut vello_gpu::Resources,
    fonts: &'a mut HashMap<(usize, u32), FontData>,
    ids: &'a HashMap<(usize, usize), (ImageId, bool)>,
    scale: f64,
}

impl Drawer<'_> {
    /// Draws `item`, in the group with `transform` if it's in one.
    fn item(&mut self, item: &Item, transform: Option<Affine>) {
        let scale = self.scale;
        let at = |v: i64| v as f64 * scale;
        match item {
            Item::Fill { path, color } => {
                self.scene.set_paint(paint(*color));
                self.scene.fill_path(&bez(path, scale));
            }
            Item::Stroke { path, width, color } => {
                self.scene.set_paint(paint(*color));
                self.scene.set_stroke(Stroke::new(at(*width)));
                self.scene.stroke_path(&bez(path, scale));
            }
            Item::Glyphs(run) => {
                let font = self
                    .fonts
                    .entry(run.font.key())
                    .or_insert_with(|| {
                        FontData::new(Blob::new(Arc::clone(&run.font.data)), run.font.index)
                    })
                    .clone();
                self.scene.set_paint(paint(run.color));
                let mut builder = self
                    .scene
                    .glyph_run(self.resources, &font)
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
            Item::Image { image, rect, .. } => {
                if rect.is_empty() {
                    return;
                }
                let (x, y) = (at(rect.x), at(rect.y));
                let (w, h) = (at(rect.width), at(rect.height));
                let index = level_of(image, rect, scale, transform.unwrap_or(Affine::IDENTITY));
                let level = &image.levels()[index];
                match self.ids.get(&(image.key(), index)) {
                    Some(&(id, opaque)) => {
                        self.scene.set_paint(vello_gpu::Image {
                            image: ImageSource::OpaqueId {
                                id,
                                may_have_transparency: !opaque,
                            },
                            sampler: ImageSampler {
                                x_extend: Extend::Pad,
                                y_extend: Extend::Pad,
                                quality: ImageQuality::Medium,
                                alpha: 1.0,
                            },
                        });
                        // The level is in the atlas with a texel of border.
                        let (lw, lh) = (f64::from(level.width), f64::from(level.height));
                        self.scene.set_paint_transform(
                            Affine::translate((x, y))
                                * Affine::scale_non_uniform(w / lw, h / lh)
                                * Affine::translate((-1.0, -1.0)),
                        );
                        self.scene.fill_rect(&kurbo::Rect::new(x, y, x + w, y + h));
                        self.scene.reset_paint_transform();
                    }
                    // More images than the atlas holds: grey.
                    None => {
                        self.scene.set_paint(paint(Color::rgb(128, 128, 128)));
                        self.scene.fill_rect(&kurbo::Rect::new(x, y, x + w, y + h));
                    }
                }
            }
            Item::Group(group) => {
                let outer = transform.unwrap_or(Affine::IDENTITY);
                let inner = outer * group_affine(&group.transform, scale);
                if let Some(clip) = &group.clip {
                    self.scene.set_transform(outer);
                    self.scene.push_clip_path(&bez(clip, scale));
                }
                for item in &group.items {
                    self.scene.set_transform(inner);
                    self.item(item, Some(inner));
                }
                self.scene.set_transform(outer);
                if group.clip.is_some() {
                    self.scene.pop_clip();
                }
            }
        }
    }
}

/// An image level as it goes into the atlas: premultiplied RGBA, with a
/// texel of border around it copied from its edge. vello_gpu holds a
/// sample to the image before it takes half a texel off to sample
/// bilinearly, so without the border its last half texel on the right and
/// at the bottom would blend the last two; with it, its edges are held
/// as vello_cpu holds them.
struct Bordered {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Bordered {
    fn new(level: &scene::Level) -> Bordered {
        let (w, h) = (level.width as usize, level.height as usize);
        let mut pixels = Vec::with_capacity((w + 2) * (h + 2) * 4);
        for y in 0..h + 2 {
            let source = (y.clamp(1, h) - 1) * w * 4;
            let line = &level.pixels[source..source + w * 4];
            pixels.extend_from_slice(&line[..4]);
            pixels.extend_from_slice(line);
            pixels.extend_from_slice(&line[(w - 1) * 4..]);
        }
        Bordered {
            width: level.width + 2,
            height: level.height + 2,
            pixels,
        }
    }
}

impl vello_gpu::AtlasWriter for Bordered {
    fn width(&self) -> u16 {
        self.width as u16
    }

    fn height(&self) -> u16 {
        self.height as u16
    }

    fn write_to_atlas(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        atlas_texture: &wgpu::Texture,
        offset: [u16; 2],
        width: u16,
        height: u16,
    ) {
        let (width, height) = (u32::from(width), u32::from(height));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: u32::from(offset[0]),
                    y: u32::from(offset[1]),
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// A window's own picture, kept between frames, in its surface's format.
/// A frame draws into it where the display list changed since the one it
/// shows, and it's copied whole to the window.
pub struct Canvas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u16,
    height: u16,
    format: wgpu::TextureFormat,
    /// Whether it's on a software GPU, such as llvmpipe.
    software: bool,
    /// What it shows, at what resolution, if it shows anything yet.
    shows: Option<(Page, f64)>,
}

/// The most of render-cpu's pixels [`Canvas::write`] has the GPU take at
/// once, in bytes.
const BAND: usize = 512 * 1024;

impl Canvas {
    /// An empty canvas `width` by `height` pixels in `format`.
    pub fn new(device: &Device, width: u16, height: u16, format: wgpu::TextureFormat) -> Canvas {
        let texture = device.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas"),
            size: wgpu::Extent3d {
                width: u32::from(width.max(1)),
                height: u32::from(height.max(1)),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Canvas {
            texture,
            view,
            width: width.max(1),
            height: height.max(1),
            format,
            software: device.adapter.get_info().device_type == wgpu::DeviceType::Cpu,
            shows: None,
        }
    }

    /// Its size, in pixels.
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Its format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// What to draw for it to show `page` at `ppi`, in pixels: where the
    /// page differs from what it shows, or all of it if it shows nothing
    /// yet or shows it at another resolution.
    pub fn damage(&self, page: &Page, ppi: f64) -> Vec<PixelRect> {
        let (w, h) = (u32::from(self.width), u32::from(self.height));
        match &self.shows {
            Some((shown, at)) if *at == ppi => scene::redraw(shown, page, ppi, w, h),
            _ => vec![PixelRect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            }],
        }
    }

    /// Draws `rects` of `page` at `ppi` into it with `renderer`, over
    /// `background`, and takes it as what it shows.
    pub fn draw(
        &mut self,
        device: &Device,
        renderer: &mut Renderer,
        page: Page,
        ppi: f64,
        background: Color,
        rects: &[PixelRect],
    ) -> Result<(), String> {
        let size = (self.width, self.height);
        let result = renderer.render_rects(device, &page, ppi, &self.view, size, background, rects);
        self.shows = Some((page, ppi));
        result
    }

    /// Writes `rgba`, straight RGBA pixels `rect` holds, as render-cpu
    /// draws them, into it at `rect`.
    ///
    /// They go a band of rows at a time, and on a software GPU each band
    /// is done before the next goes: there the memory a write is staged
    /// in is the program's own, and stays resident once used, so a whole
    /// window at once would keep megabytes of it.
    pub fn write(&mut self, device: &Device, rect: PixelRect, rgba: &[u8]) {
        let (w, h) = (
            rect.width.min(u32::from(self.width).saturating_sub(rect.x)),
            rect.height
                .min(u32::from(self.height).saturating_sub(rect.y)),
        );
        if w == 0 || h == 0 || rgba.len() < (rect.width * rect.height * 4) as usize {
            return;
        }
        let bgra = matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let (row, inside) = (rect.width as usize * 4, w as usize * 4);
        let rows = (BAND / inside).clamp(1, h as usize);
        for (k, band) in rgba[..row * h as usize].chunks(row * rows).enumerate() {
            let lines = (band.len() / row) as u32;
            // Whole rows of RGBA go as they are; others are copied a row
            // at a time, and their red and blue swapped in place for BGRA:
            // not a pixel at a time, which costs milliseconds on a whole
            // window.
            let pixels: Cow<'_, [u8]> = if !bgra && row == inside {
                Cow::Borrowed(band)
            } else {
                let mut pixels = Vec::with_capacity(inside * lines as usize);
                for line in band.chunks_exact(row) {
                    pixels.extend_from_slice(&line[..inside]);
                }
                if bgra {
                    for pixel in pixels.as_chunks_mut::<4>().0 {
                        pixel.swap(0, 2);
                    }
                }
                Cow::Owned(pixels)
            };
            device.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: rect.x,
                        y: rect.y + (k * rows) as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(lines),
                },
                wgpu::Extent3d {
                    width: w,
                    height: lines,
                    depth_or_array_layers: 1,
                },
            );
            if self.software {
                device.queue.submit([]);
                let _ = device.device.poll(wgpu::PollType::wait_indefinitely());
            }
        }
    }

    /// Takes `page` at `ppi` as what it shows, once render-cpu's pixels of
    /// it are written.
    pub fn shows(&mut self, page: Page, ppi: f64) {
        self.shows = Some((page, ppi));
    }

    /// Forgets what it shows, so the next frame draws all of it: when
    /// another renderer takes over.
    pub fn forget(&mut self) {
        self.shows = None;
    }

    /// Copies it to `target`, a window's frame of its format, as far as
    /// both reach, and submits the copy.
    pub fn copy_to(&self, device: &Device, target: &wgpu::Texture) {
        let mut encoder = device.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            target.as_image_copy(),
            wgpu::Extent3d {
                width: u32::from(self.width).min(target.width()),
                height: u32::from(self.height).min(target.height()),
                depth_or_array_layers: 1,
            },
        );
        device.queue.submit([encoder.finish()]);
    }

    /// Its pixels, read back: as [`Renderer::render_offscreen`] gives
    /// them, for tests.
    pub fn read(&self, device: &Device) -> Vec<u8> {
        read(device, &self.texture, self.width, self.height)
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
