//! The chrome's look as data (`spec/themes/`): its type and sizes, the same
//! in light and dark, and each look's colours by role.

use scene::Color;
use serde::{Deserialize, Deserializer};
use text::units::Length;

/// The chrome's type and sizes, as `spec/themes/chrome.ron` gives them;
/// each is described there.
#[derive(Clone, Debug, Deserialize)]
#[allow(missing_docs, reason = "each is described in spec/themes/chrome.ron")]
pub struct Sizes {
    pub family: String,
    pub text: Length,
    pub label: Length,
    pub value: Length,
    pub heading: Length,
    pub tiny: Length,
    pub palette_text: Length,
    pub palette_item: Length,
    pub title_bar: Length,
    pub menu_left: Length,
    pub menu_gap: Length,
    pub window_button: Length,
    pub button: Length,
    pub contextual_gap: Length,
    pub segmented: (Length, Length),
    pub menu_item: Length,
    pub menu_separator: Length,
    pub menu_padding: Length,
    pub resize_edge: Length,
    pub ruler: Length,
    pub status_bar: Length,
    pub inspector: Length,
    pub inspector_padding: Length,
    pub inspector_label: Length,
    pub inspector_row: Length,
    pub field: Length,
    pub section_gap: Length,
    pub radius: Length,
    pub palette: Length,
    pub palette_field: Length,
    pub palette_row: Length,
    pub palette_top: Length,
    pub page_top: Length,
    pub page_gap: Length,
    pub draft_column: Length,
    pub draft_top: Length,
}

/// A look's colours by role, as `spec/themes/light.ron` and `dark.ron`
/// give them; each is described there.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[allow(missing_docs, reason = "each is described in spec/themes/light.ron")]
pub struct Colors {
    #[serde(deserialize_with = "color")]
    pub desk: Color,
    #[serde(deserialize_with = "color")]
    pub chrome: Color,
    #[serde(deserialize_with = "color")]
    pub line: Color,
    #[serde(deserialize_with = "color")]
    pub ink: Color,
    #[serde(deserialize_with = "color")]
    pub secondary: Color,
    #[serde(deserialize_with = "color")]
    pub disabled: Color,
    #[serde(deserialize_with = "color")]
    pub hover: Color,
    #[serde(deserialize_with = "color")]
    pub accent: Color,
    #[serde(deserialize_with = "color")]
    pub field: Color,
    #[serde(deserialize_with = "color")]
    pub field_line: Color,
    #[serde(deserialize_with = "color")]
    pub shadow: Color,
    #[serde(deserialize_with = "color")]
    pub selection: Color,
    #[serde(deserialize_with = "color")]
    pub formatting: Color,
    #[serde(deserialize_with = "color")]
    pub danger: Color,
    #[serde(deserialize_with = "color")]
    pub page: Color,
    #[serde(deserialize_with = "color")]
    pub page_ink: Color,
}

/// A colour as specs write it: red, green and blue, and opacity, each 0 to
/// 255.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ColorSpec {
    /// Opaque.
    Rgb(u8, u8, u8),
    /// With its opacity.
    Rgba(u8, u8, u8, u8),
}

impl ColorSpec {
    /// The colour.
    pub fn color(self) -> Color {
        match self {
            ColorSpec::Rgb(r, g, b) => Color::rgb(r, g, b),
            ColorSpec::Rgba(r, g, b, a) => Color { r, g, b, a },
        }
    }
}

fn color<'de, D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
    ColorSpec::deserialize(d).map(ColorSpec::color)
}

/// The chrome's look: its sizes, and the colours of the look it's in,
/// light or dark.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Its type and sizes.
    pub sizes: Sizes,
    /// Its colours.
    pub colors: Colors,
}

impl Theme {
    /// `sizes` in `colors`.
    pub fn new(sizes: Sizes, colors: Colors) -> Theme {
        Theme { sizes, colors }
    }
}
