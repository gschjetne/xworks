//! render-gpu draws what render-cpu draws, within a few levels of 255
//! (docs/ARCHITECTURE.md, section 10). It needs a GPU, so it's ignored
//! unless asked for: on WSL, run the Windows build (CLAUDE.md).

use scene::{Color, Item, Page, Path, Point, Segment};
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
