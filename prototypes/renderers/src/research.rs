//! Vello's compute-centric GPU renderer, `vello` ("Vello Research").

use vello::kurbo::{Affine, Stroke};
use vello::peniko::{Blob, Brush, Color, Fill, FontData, Gradient};

use crate::corpus::{Fonts, Item, Page, Paint, Rgba};

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}

fn brush(paint: &Paint) -> Brush {
    let stops = |stops: &[(f32, Rgba)]| -> Vec<(f32, Color)> {
        stops.iter().map(|&(t, c)| (t, color(c))).collect()
    };
    match paint {
        Paint::Solid(c) => Brush::Solid(color(*c)),
        Paint::Linear { start, end, stops: s } => {
            Gradient::new_linear(*start, *end).with_stops(stops(s).as_slice()).into()
        }
        Paint::Radial { center, radius, stops: s } => {
            Gradient::new_radial(*center, *radius).with_stops(stops(s).as_slice()).into()
        }
    }
}

pub fn font_data(fonts: &Fonts) -> Vec<FontData> {
    fonts
        .data
        .iter()
        .map(|d| FontData::new(Blob::new(d.clone()), 0))
        .collect()
}

/// The scene for `page`.
pub fn scene(page: &Page, fonts: &[FontData]) -> vello::Scene {
    let mut scene = vello::Scene::new();
    for item in &page.items {
        match item {
            Item::Fill { path, paint } => {
                scene.fill(Fill::NonZero, Affine::IDENTITY, &brush(paint), None, path);
            }
            Item::Stroke { path, width, paint } => {
                scene.stroke(
                    &Stroke::new(*width),
                    Affine::IDENTITY,
                    &brush(paint),
                    None,
                    path,
                );
            }
            Item::Glyphs { run } => {
                scene
                    .draw_glyphs(&fonts[run.font])
                    .font_size(run.size)
                    .hint(false)
                    .brush(color(run.color))
                    .draw(
                        Fill::NonZero,
                        run.glyphs
                            .iter()
                            .map(|&(id, x, y)| vello::Glyph { id, x, y }),
                    );
            }
        }
    }
    scene
}

/// The renderer, compiling its shaders for area antialiasing only.
pub fn renderer(device: &wgpu::Device) -> vello::Renderer {
    vello::Renderer::new(
        device,
        vello::RendererOptions {
            use_cpu: false,
            antialiasing_support: vello::AaSupport::area_only(),
            num_init_threads: None,
            pipeline_cache: None,
        },
    )
    .expect("vello's renderer starts")
}

pub fn render(
    renderer: &mut vello::Renderer,
    page: &Page,
    scene: &vello::Scene,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
) {
    renderer
        .render_to_texture(
            device,
            queue,
            scene,
            view,
            &vello::RenderParams {
                base_color: color([255, 255, 255, 255]),
                width: page.width as u32,
                height: page.height as u32,
                antialiasing_method: vello::AaConfig::Area,
            },
        )
        .expect("vello renders");
}
