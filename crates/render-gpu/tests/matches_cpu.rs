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
