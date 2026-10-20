//! Layout units: fixed-point integers, 914 400 to the inch
//! (docs/ARCHITECTURE.md, section 9). Points, millimetres and 96-dpi pixels
//! are all whole numbers of them, and integer layout breaks lines and pages
//! the same way on every platform.

/// An inch.
pub const INCH: i64 = 914_400;
/// A typographic point, 1/72 inch.
pub const POINT: i64 = 12_700;
/// A millimetre.
pub const MILLIMETRE: i64 = 36_000;
/// A pixel at 96 dots to the inch.
pub const PIXEL: i64 = 9_525;

/// `value` font units, at `size` layout units to the em of a font with
/// `units_per_em`, in layout units: rounded to the nearest, halves away
/// from zero.
pub(crate) fn scale(value: i32, size: i64, units_per_em: u16) -> i64 {
    let n = i64::from(value) * size;
    let d = i64::from(units_per_em.max(1));
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_divide_evenly() {
        assert_eq!(INCH, 72 * POINT);
        assert_eq!(INCH * 10, 254 * MILLIMETRE);
        assert_eq!(INCH, 96 * PIXEL);
    }

    #[test]
    fn scaling_rounds_halves_away_from_zero() {
        // 1069 units of a 1000-unit em at 12 points: 162 915.6.
        assert_eq!(scale(1069, 12 * POINT, 1000), 162_916);
        assert_eq!(scale(-293, 12 * POINT, 1000), -44_653);
        assert_eq!(scale(1, 500, 1000), 1);
        assert_eq!(scale(-1, 500, 1000), -1);
        assert_eq!(scale(0, 500, 1000), 0);
    }
}
