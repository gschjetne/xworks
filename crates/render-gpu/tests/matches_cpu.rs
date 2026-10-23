//! render-gpu draws what render-cpu draws, within a few levels of 255
//! (docs/ARCHITECTURE.md, section 10). It needs a GPU, so it's ignored
//! unless asked for: on WSL, run the Windows build (CLAUDE.md).

use scene::{Color, Group, Item, Page, Path, Point, Rect, Segment, Transform};
use text::units::{INCH, POINT};
use text::{Align, Fonts, Language, Paragraph, Span, Style};

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
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
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
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
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
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
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
    let shown = canvas.read(&device);
    let (_, _, whole) = renderer
        .render_offscreen(&device, &new, ppi)
        .expect("it renders");
    let worst = worst(&shown, &whole);
    println!("the canvas is {worst} levels from the new page drawn whole");
    assert!(worst <= 1, "{worst} levels apart");
}

/// What render-cpu draws is written into a canvas as it is, in either of
/// the formats windows' frames have, whole or where it changed, and a
/// rectangle reaching past the canvas is cut at its edge.
#[test]
#[ignore = "needs a GPU: on WSL, run the Windows build (CLAUDE.md)"]
fn render_cpus_pixels_are_written_as_they_are() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let device = render_gpu::open(&instance, None).expect("a GPU");
    let (w, h) = (37u32, 23u32);
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
        let mut canvas = render_gpu::Canvas::new(&device, w as u16, h as u16, format);
        let whole: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| pixel(x, y, 0)))
            .collect();
        canvas.write(&device, rect(0, 0, w, h), &whole);
        // A part, 10 by 5 from (30, 4): only 7 of its columns are inside.
        let part: Vec<u8> = (0..5)
            .flat_map(|y| (0..10).flat_map(move |x| pixel(x, y, 1)))
            .collect();
        canvas.write(&device, rect(30, 4, 10, 5), &part);
        let mut expected = whole.clone();
        for y in 0..5 {
            for x in 0..7 {
                let at = (((4 + y) * w + 30 + x) * 4) as usize;
                expected[at..at + 4].copy_from_slice(&pixel(x, y, 1));
            }
        }
        if format == wgpu::TextureFormat::Bgra8Unorm {
            for p in expected.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
            }
        }
        let got = canvas.read(&device);
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
