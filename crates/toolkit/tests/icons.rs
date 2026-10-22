//! Lucide's icons (`icons/lucide/`) read, and draw inside their square.

use scene::{Color, Item, Segment};
use toolkit::Icon;

#[test]
fn every_icon_reads_and_draws_inside_its_square() {
    let icons = testkit::icons().expect("icons/lucide");
    assert!(!icons.is_empty(), "no icons in icons/lucide");
    for (name, svg) in &icons {
        let icon = Icon::from_svg(svg).unwrap_or_else(|e| panic!("{name}: {e}"));
        // 24 pixels at 96 to the inch, at (1000, 1000).
        let size = 24 * text::units::PIXEL;
        let Item::Fill { path: drawn, .. } = icon.draw(1000, 1000, size, Color::BLACK) else {
            panic!("an icon is a fill");
        };
        assert!(!drawn.0.is_empty(), "{name}");
        for segment in &drawn.0 {
            let points = match segment {
                Segment::MoveTo(p) | Segment::LineTo(p) => vec![*p],
                Segment::QuadTo(a, b) => vec![*a, *b],
                Segment::CubicTo(a, b, c) => vec![*a, *b, *c],
                Segment::Close => Vec::new(),
            };
            for p in points {
                assert!(
                    (1000..=1000 + size).contains(&p.x) && (1000..=1000 + size).contains(&p.y),
                    "{name}: {p:?} is outside its square"
                );
            }
        }
    }
}
