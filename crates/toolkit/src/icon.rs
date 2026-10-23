//! Icons: Lucide's icons (`icons/lucide/`), read from their SVG and drawn
//! as fills, so every renderer draws them as it draws any other shape.
//!
//! A Lucide icon is a 24-unit square of lines, two units wide, with round
//! ends and joins. [`Icon::from_svg`] reads the elements Lucide uses:
//! `path`, `line`, `rect` and `circle`. [`Icon::draw`] strokes them at the
//! size asked for.

use kurbo::{Affine, BezPath, Cap, Circle, Join, Line, PathEl, RoundedRect, Shape, Stroke};
use scene::{Color, Item, Path, Point, Segment};

/// The square an icon is drawn in, in its own units.
const GRID: f64 = 24.0;

/// How wide its lines are, in its own units.
const LINE: f64 = 2.0;

/// How closely curves are followed when stroking, in the icon's units.
const TOLERANCE: f64 = 0.01;

/// Why an icon couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum IconError {
    /// An element lacks an attribute it needs, or has one that isn't a
    /// number.
    #[error("<{element}>: {attribute} is missing or not a number")]
    Attribute {
        /// The element.
        element: String,
        /// The attribute.
        attribute: String,
    },
    /// A path's data can't be read.
    #[error("<path d=\"{0}\">: not path data")]
    PathData(String),
    /// The icon draws nothing.
    #[error("no path, line, rect or circle")]
    Empty,
}

/// An icon: the centre lines of its strokes, in a 24-unit square.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    lines: BezPath,
}

impl Icon {
    /// Reads a Lucide icon's SVG.
    pub fn from_svg(svg: &str) -> Result<Icon, IconError> {
        let mut lines = BezPath::new();
        for (name, attributes) in elements(svg) {
            let number = |key: &str| {
                attribute(attributes, key)
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or_else(|| IconError::Attribute {
                        element: name.to_string(),
                        attribute: key.to_string(),
                    })
            };
            match name {
                "path" => {
                    let d = attribute(attributes, "d").unwrap_or_default();
                    let path =
                        BezPath::from_svg(d).map_err(|_| IconError::PathData(d.to_string()))?;
                    lines.extend(path);
                }
                "line" => {
                    let line = Line::new(
                        (number("x1")?, number("y1")?),
                        (number("x2")?, number("y2")?),
                    );
                    lines.extend(line.path_elements(TOLERANCE));
                }
                "rect" => {
                    let (x, y) = (number("x")?, number("y")?);
                    let rx = number("rx").unwrap_or(0.0);
                    let rect =
                        RoundedRect::new(x, y, x + number("width")?, y + number("height")?, rx);
                    lines.extend(rect.path_elements(TOLERANCE));
                }
                "circle" => {
                    let circle = Circle::new((number("cx")?, number("cy")?), number("r")?);
                    lines.extend(circle.path_elements(TOLERANCE));
                }
                _ => {}
            }
        }
        if lines.elements().is_empty() {
            return Err(IconError::Empty);
        }
        Ok(Icon { lines })
    }

    /// The icon drawn `size` across, with its top left at (`x`, `y`), in
    /// `color`. Its lines are as wide as Lucide's at that size: a twelfth
    /// of it.
    pub fn draw(&self, x: i64, y: i64, size: i64, color: Color) -> Item {
        let stroke = Stroke::new(LINE)
            .with_caps(Cap::Round)
            .with_join(Join::Round);
        let outline = kurbo::stroke(
            self.lines.iter(),
            &stroke,
            &kurbo::StrokeOpts::default(),
            TOLERANCE,
        );
        let place = Affine::translate((x as f64, y as f64)) * Affine::scale(size as f64 / GRID);
        Item::Fill {
            path: path(&(place * outline)),
            color,
        }
    }
}

/// `path` in layout units, rounded to the nearest.
fn path(path: &BezPath) -> Path {
    let at = |p: kurbo::Point| Point::new(p.x.round() as i64, p.y.round() as i64);
    Path(
        path.elements()
            .iter()
            .map(|el| match *el {
                PathEl::MoveTo(p) => Segment::MoveTo(at(p)),
                PathEl::LineTo(p) => Segment::LineTo(at(p)),
                PathEl::QuadTo(a, b) => Segment::QuadTo(at(a), at(b)),
                PathEl::CurveTo(a, b, c) => Segment::CubicTo(at(a), at(b), at(c)),
                PathEl::ClosePath => Segment::Close,
            })
            .collect(),
    )
}

/// The elements of `svg`, each its name and its attributes' text: enough
/// of XML for an icon's flat list of shapes.
fn elements(svg: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else {
            break;
        };
        let tag = rest[..end].trim_end_matches('/').trim();
        rest = &rest[end + 1..];
        if tag.starts_with(['/', '!', '?']) {
            continue;
        }
        let (name, attributes) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        out.push((name, attributes));
    }
    out
}

/// The value of attribute `key` in `attributes`, if it's there.
fn attribute<'a>(attributes: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = attributes;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        let value_and_rest = &after[quote.len_utf8()..];
        let close = value_and_rest.find(quote)?;
        if name == key {
            return Some(&value_and_rest[..close]);
        }
        rest = &value_and_rest[close + 1..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_found_by_name() {
        let attrs = r#"x1="19" x2="10" y1="4" y2="4""#;
        assert_eq!(attribute(attrs, "x2"), Some("10"));
        assert_eq!(attribute(attrs, "y1"), Some("4"));
        assert_eq!(attribute(attrs, "r"), None);
    }

    #[test]
    fn an_svg_without_shapes_is_refused() {
        assert!(matches!(
            Icon::from_svg("<svg></svg>"),
            Err(IconError::Empty)
        ));
        assert!(matches!(
            Icon::from_svg(r#"<svg><line x1="1" /></svg>"#),
            Err(IconError::Attribute { .. })
        ));
    }
}
