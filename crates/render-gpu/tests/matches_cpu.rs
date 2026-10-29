//! render-gpu draws what render-cpu draws, within a few levels of 255
//! (docs/ARCHITECTURE.md, section 10). It needs a GPU, so it's ignored
//! unless asked for: on WSL, run the Windows build (CLAUDE.md).

use scene::{Color, Group, Item, Page, Path, Point, Rect, Segment, Transform};
use text::units::{INCH, POINT};
use text::{Align, Fonts, Language, Paragraph, Span, Style};

/// An instance on Vulkan, Metal or DirectX 12, unless `WGPU_BACKEND` asks
/// for others, as the shells open it. Not OpenGL: on WSL, starting it has
/// Mesa load and unload WSL's `libd3d12core.so`, which leaves a destructor
/// for the thread's data pointing where it was, so the test's thread
/// crashes as it ends.
fn instance() -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.backends = wgpu::Backends::PRIMARY.with_env();
    wgpu::Instance::new(descriptor)
}

fn page() -> Page {
    let mut fonts = Fonts::new();
    for (name, bytes) in testkit::fonts::load().expect("the bundled fonts") {
        fonts.add(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let spec: text::FontSpec =
        ron::from_str(&testkit::fonts::spec().expect("fonts.ron")).expect("valid");
    fonts.set_fallbacks(&spec);
    let mut page = Page::new(4 * INCH, 3 * INCH);
    page.fill_background(Color::WHITE);
    page.items.push(Item::Fill {
        path: Path::rect(INCH / 4, INCH / 4, INCH, INCH / 2),
        color: Color::rgb(32, 48, 96),
    });
    page.items.push(Item::Stroke {
        path: Path(vec![
            Segment::MoveTo(Point::new(INCH / 4, 2 * INCH)),
            Segment::QuadTo(
                Point::new(2 * INCH, INCH),
                Point::new(15 * INCH / 4, 2 * INCH),
            ),
        ]),
        width: 3 * POINT,
        color: Color {
            r: 30,
            g: 130,
            b: 60,
            a: 200,
        },
    });
    let text = "Glyphs on the GPU: Ελληνικά, Кириллица, שלום, مرحبا, 日本語.";
    let spans = [Span {
        len: text.len(),
        style: Style {
            family: "Noto Serif".to_string(),
            size: 11 * POINT,
            weight: 400,
            italic: true,
            language: Language::und(),
            ligatures: text::Ligatures::default(),
        },
    }];
    let layout = text::layout(
        &fonts,
        &Paragraph {
            text,
            spans: &spans,
            direction: None,
            align: Align::Start,
            width: 7 * INCH / 2,
        },
    );
    page.items
        .extend(layout.paint(&fonts, Point::new(INCH / 4, INCH), None, |_| Color::BLACK));
    page
}

#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn the_gpu_draws_what_the_cpu_draws() {
    let instance = instance();
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let page = page();
    let mut renderer = render_gpu::Renderer::new(&device.device, wgpu::TextureFormat::Rgba8Unorm);
    let (w, h, gpu) = renderer
        .render_offscreen(&device, &page, 144.0)
        .expect("it renders");
    let cpu = render_cpu::render(&page, 144.0, render_cpu::Level::Portable);
    assert_eq!((u32::from(w), u32::from(h)), (cpu.width, cpu.height));
    // The page is opaque, so premultiplied and straight pixels are the same.
    let worst = gpu
        .iter()
        .zip(&cpu.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    let info = device.adapter.get_info();
    assert!(
        worst <= 8,
        "{} ({:?}): {worst} levels apart",
        info.name,
        info.backend
    );
}

/// A `width` by `height` picture of colour bands and a soft-edged spot.
fn picture(width: u32, height: u32, transparent: bool) -> scene::Image {
    let mut rgba = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x * 255 / width, y * 255 / height);
            let band = if (x / 7 + y / 5) % 2 == 0 { 40 } else { 220 };
            let a = if transparent { (u + v) / 2 } else { 255 };
            rgba.extend_from_slice(&[u as u8, v as u8, band, a as u8]);
        }
    }
    scene::Image::new(width, height, rgba).expect("an image")
}

/// Images large and small, clipped and turned, over text.
fn pictures() -> Page {
    let mut page = page();
    let photo = picture(640, 400, false);
    // Drawn small, from a smaller level; drawn large, stretched.
    page.items.push(Item::Image {
        image: photo.clone(),
        rect: Rect::new(INCH / 4, INCH / 4, 3 * INCH / 4, INCH / 2),
        alt: None,
        source: None,
    });
    page.items.push(Item::Image {
        image: picture(12, 9, true),
        rect: Rect::new(5 * INCH / 4, INCH / 4, INCH, 3 * INCH / 4),
        alt: None,
        source: None,
    });
    page.items.push(Item::Group(Group {
        clip: Some(Path::rect(2 * INCH, 3 * INCH / 2, 2 * INCH, 3 * INCH / 2)),
        transform: Transform([0.8, 0.6, -0.6, 0.8, 3.0 * INCH as f64, 1.3 * INCH as f64]),
        items: vec![
            Item::Image {
                image: photo,
                rect: Rect::new(0, 0, 3 * INCH / 2, 15 * INCH / 16),
                alt: None,
                source: None,
            },
            Item::Fill {
                path: Path::rect(0, 0, INCH / 2, INCH / 4),
                color: Color {
                    r: 200,
                    g: 40,
                    b: 40,
                    a: 160,
                },
            },
        ],
    }));
    page
}

fn worst(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn images_clips_and_transforms_draw_as_on_the_cpu() {
    let instance = instance();
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let page = pictures();
    let mut renderer = render_gpu::Renderer::new(&device.device, wgpu::TextureFormat::Rgba8Unorm);
    let (w, h, gpu) = renderer
        .render_offscreen(&device, &page, 144.0)
        .expect("it renders");
    let cpu = render_cpu::render(&page, 144.0, render_cpu::Level::Portable);
    assert_eq!((u32::from(w), u32::from(h)), (cpu.width, cpu.height));
    let worst = worst(&gpu, &cpu.pixels);
    let info = device.adapter.get_info();
    println!("{} ({:?}): {worst} levels apart", info.name, info.backend);
    assert!(
        worst <= 8,
        "{} ({:?}): {worst} levels apart",
        info.name,
        info.backend
    );
}

#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn a_canvas_drawn_where_it_changed_shows_the_new_page() {
    let instance = instance();
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = render_gpu::Renderer::new(&device.device, format);
    let ppi = 144.0;
    let old = pictures();
    let mut new = pictures();
    // A caret appears and a picture moves.
    new.items.push(Item::Fill {
        path: Path::rect(INCH, 5 * INCH / 2, INCH / 64, INCH / 6),
        color: Color::BLACK,
    });
    let small = new.items.len() - 3;
    if let Item::Image { rect, .. } = &mut new.items[small] {
        rect.x += INCH / 3;
    }
    let (w, h) = (
        render_gpu::pixels(old.width, ppi),
        render_gpu::pixels(old.height, ppi),
    );
    let mut canvas = render_gpu::Canvas::new(&device, w, h, format);
    let all = canvas.damage(&old, ppi);
    assert_eq!(all.len(), 1, "a new canvas draws everything");
    canvas
        .draw(&device, &mut renderer, old, ppi, Color::WHITE, &all)
        .expect("it draws");
    let rects = canvas.damage(&new, ppi);
    let area: u32 = rects.iter().map(|r| r.width * r.height).sum();
    assert!(
        area * 2 < u32::from(w) * u32::from(h),
        "only a part: {rects:?}"
    );
    canvas
        .draw(
            &device,
            &mut renderer,
            new.clone(),
            ppi,
            Color::WHITE,
            &rects,
        )
        .expect("it draws");
    let shown = canvas.read(&device).expect("it reads back");
    let (_, _, whole) = renderer
        .render_offscreen(&device, &new, ppi)
        .expect("it renders");
    let worst = worst(&shown, &whole);
    println!("the canvas is {worst} levels from the new page drawn whole");
    assert!(worst <= 1, "{worst} levels apart");
}

/// What render-cpu draws is written into a canvas as it is, in either of
/// the formats windows' frames have, whole or where it changed, and a
/// rectangle reaching past the canvas is cut at its edge: in several
/// bands, as large ones go, on the GPU and on the software one where
/// there's one, such as Windows' WARP, where each band waits for the last.
#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn render_cpus_pixels_are_written_as_they_are() {
    let instance = instance();
    let gpu = render_gpu::open(&instance, None).expect("a GPU");
    let software = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        force_fallback_adapter: true,
        ..Default::default()
    }))
    .ok()
    .and_then(|adapter| {
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some(render_gpu::Device {
            adapter,
            device,
            queue,
        })
    });
    for device in std::iter::once(gpu).chain(software) {
        let info = device.adapter.get_info();
        println!("{} ({:?})", info.name, info.device_type);
        written_as_they_are(&device);
    }
}

fn written_as_they_are(device: &render_gpu::Device) {
    let (w, h) = (400u32, 900u32);
    let pixel = |x: u32, y: u32, n: u32| {
        [
            (x * 7 + n) as u8,
            (y * 11 + n) as u8,
            (x + y + n) as u8,
            (200 + n) as u8,
        ]
    };
    let rect = |x, y, width, height| scene::PixelRect {
        x,
        y,
        width,
        height,
    };
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8Unorm,
    ] {
        let mut canvas = render_gpu::Canvas::new(device, w as u16, h as u16, format);
        let whole: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| pixel(x, y, 0)))
            .collect();
        canvas.write(device, rect(0, 0, w, h), &whole);
        // A part, 400 by 890 from (100, 4): only 300 of its columns are
        // inside.
        let part: Vec<u8> = (0..890)
            .flat_map(|y| (0..400).flat_map(move |x| pixel(x, y, 1)))
            .collect();
        canvas.write(device, rect(100, 4, 400, 890), &part);
        let mut expected = whole.clone();
        for y in 0..890 {
            for x in 0..300 {
                let at = (((4 + y) * w + 100 + x) * 4) as usize;
                expected[at..at + 4].copy_from_slice(&pixel(x, y, 1));
            }
        }
        if format == wgpu::TextureFormat::Bgra8Unorm {
            for p in expected.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
            }
        }
        let got = canvas.read(device).expect("it reads back");
        let first = got.iter().zip(&expected).position(|(a, b)| a != b);
        assert!(
            first.is_none(),
            "{format:?} on {}: byte {first:?} of {}: got {:?}, expected {:?}",
            device.adapter.get_info().name,
            got.len(),
            first.map(|i| &got[i / 4 * 4..i / 4 * 4 + 4]),
            first.map(|i| &expected[i / 4 * 4..i / 4 * 4 + 4]),
        );
    }
}

/// Solid images, one per cell of a row of cells a tenth of an inch wide,
/// and how to read them back.
struct Cells {
    images: Vec<scene::Image>,
}

impl Cells {
    const PPI: f64 = 100.0;

    /// `count` images, each a colour of its own, starting from `first`.
    fn new(first: usize, count: usize) -> Cells {
        Cells {
            images: (first..first + count)
                .map(|i| scene::Image::new(2, 2, Cells::colour(i).repeat(4)).expect("an image"))
                .collect(),
        }
    }

    fn colour(i: usize) -> [u8; 4] {
        [(i * 3) as u8, 255 - (i * 3) as u8, 40, 255]
    }

    /// A page of `cells` cells holding these images from the first.
    fn page(&self, cells: usize) -> Page {
        let mut page = Page::new(cells as i64 * INCH / 10, INCH / 10);
        for (i, image) in self.images.iter().enumerate().take(cells) {
            page.items.push(Item::Image {
                image: image.clone(),
                rect: Rect::new(i as i64 * INCH / 10, 0, INCH / 10, INCH / 10),
                alt: None,
                source: None,
            });
        }
        page
    }

    /// The colours in the middle of each of `cells` cells of `page` drawn.
    fn drawn(
        renderer: &mut render_gpu::Renderer,
        device: &render_gpu::Device,
        page: &Page,
        cells: usize,
    ) -> Vec<[u8; 4]> {
        let (w, _, pixels) = renderer
            .render_offscreen(device, page, Cells::PPI)
            .expect("it renders");
        (0..cells)
            .map(|i| {
                let at = (5 * usize::from(w) + i * 10 + 5) * 4;
                [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
            })
            .collect()
    }
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
}

/// More images than the atlas holds at once draw without an error, frame
/// after frame: the first [`render_gpu::MOST_IMAGES`] as they are, the
/// rest grey; and a page of fewer of them shows every one.
#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn more_images_than_the_atlas_holds_draw_grey() {
    let instance = instance();
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let mut renderer = render_gpu::Renderer::new(&device.device, wgpu::TextureFormat::Rgba8Unorm);
    let count = render_gpu::MOST_IMAGES + 2;
    let cells = Cells::new(0, count);
    for _ in 0..2 {
        let drawn = Cells::drawn(&mut renderer, &device, &cells.page(count), count);
        for (i, &colour) in drawn.iter().enumerate() {
            let want = if i < render_gpu::MOST_IMAGES {
                Cells::colour(i)
            } else {
                [128, 128, 128, 255]
            };
            assert!(near(colour, want), "cell {i}: {colour:?} for {want:?}");
        }
    }
    let drawn = Cells::drawn(&mut renderer, &device, &cells.page(4), 4);
    for (i, &colour) in drawn.iter().enumerate() {
        assert!(near(colour, Cells::colour(i)), "cell {i}: {colour:?}");
    }
}

/// An image put in the atlas in a frame where another left it, as when
/// one page of pictures follows another, shows as it is: vello_gpu clears
/// the space an image leaves with a pass in the frame's encoder, so the
/// new one is copied in by a later command in that encoder, not written
/// with `Queue::write_texture`, which runs before the encoder's passes and
/// would be cleared.
#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn an_image_put_where_another_just_left_shows() {
    let instance = instance();
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let mut renderer = render_gpu::Renderer::new(&device.device, wgpu::TextureFormat::Rgba8Unorm);
    let before = Cells::new(100, 4);
    Cells::drawn(&mut renderer, &device, &before.page(4), 4);
    // More than the atlas holds come next, so the four before leave it in
    // the frame the new ones come in.
    let count = render_gpu::MOST_IMAGES + 2;
    let after = Cells::new(0, count);
    let drawn = Cells::drawn(&mut renderer, &device, &after.page(count), count);
    for (i, &colour) in drawn.iter().enumerate().take(render_gpu::MOST_IMAGES) {
        assert!(near(colour, Cells::colour(i)), "cell {i}: {colour:?}");
    }
}

/// Images coming into view, frame after frame, as images unseen for a
/// while leave the atlas, show as they are: a row of pictures scrolled a
/// cell a frame, four in view, so from the 34th frame on one leaves the
/// atlas in each frame a new one comes into it, never more than it holds.
#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn images_coming_into_view_as_unseen_ones_leave_show() {
    let instance = instance();
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let mut renderer = render_gpu::Renderer::new(&device.device, wgpu::TextureFormat::Rgba8Unorm);
    let row = Cells::new(0, 80);
    for first in 0..row.images.len() - 4 {
        let view = Cells {
            images: row.images[first..first + 4].to_vec(),
        };
        let drawn = Cells::drawn(&mut renderer, &device, &view.page(4), 4);
        for (i, &colour) in drawn.iter().enumerate() {
            let want = Cells::colour(first + i);
            assert!(
                near(colour, want),
                "frame {first}, cell {i}: {colour:?} for {want:?}"
            );
        }
    }
}
