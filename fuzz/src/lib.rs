//! What the fuzz targets share.

use app::picture::{self, Format, Limits};

/// Decodes `data` as a picture file of `format`, within the limits
/// `spec/pictures.ron` sets, and checks what a picture that decodes holds.
pub fn picture(format: Format, data: &[u8]) {
    static LIMITS: std::sync::OnceLock<Limits> = std::sync::OnceLock::new();
    let limits = LIMITS.get_or_init(|| {
        ron::from_str(include_str!("../../spec/pictures.ron")).expect("spec/pictures.ron")
    });
    let Ok(decoded) = picture::decode_as(format, data, limits) else {
        return;
    };
    assert!(u64::from(decoded.width) * u64::from(decoded.height) <= limits.most_pixels);
    let image = &decoded.image;
    assert!(image.width() <= scene::MAX_SIDE && image.height() <= scene::MAX_SIDE);
    assert!(image.width() <= decoded.width && image.height() <= decoded.height);
    let last = image.levels().last().expect("a level");
    assert_eq!((last.width, last.height), (1, 1));
}
