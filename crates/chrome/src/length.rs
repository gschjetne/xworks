//! Lengths as people read and type them: a number and a unit, or picas
//! and points as typographers write them, "3p6".

use serde::{Deserialize, Serialize};

/// A unit lengths are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Unit {
    /// Typographic points, 1/72 inch: type sizes.
    Point,
    /// Centimetres.
    Centimetre,
    /// Millimetres.
    Millimetre,
    /// Inches.
    Inch,
    /// Picas, 12 points or 1/6 inch, shown with their points: "3p6".
    Pica,
}

/// Layout units to the inch (docs/ARCHITECTURE.md, section 9).
const INCH: i64 = 914_400;

impl Unit {
    /// How many layout units it is.
    pub const fn units(self) -> i64 {
        match self {
            Unit::Point => INCH / 72,
            Unit::Centimetre => INCH * 10 / 254 * 10,
            Unit::Millimetre => INCH * 10 / 254,
            Unit::Inch => INCH,
            Unit::Pica => INCH / 6,
        }
    }

    /// What follows a number in it: "pt", "cm", "mm", "in" or "pc".
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Point => "pt",
            Unit::Centimetre => "cm",
            Unit::Millimetre => "mm",
            Unit::Inch => "in",
            Unit::Pica => "pc",
        }
    }

    fn from_suffix(suffix: &str) -> Option<Unit> {
        Some(match suffix.to_lowercase().as_str() {
            "pt" => Unit::Point,
            "cm" => Unit::Centimetre,
            "mm" => Unit::Millimetre,
            "in" | "\"" | "″" => Unit::Inch,
            "pc" => Unit::Pica,
            _ => return None,
        })
    }

    /// How many decimals a length in it shows: thousandths of an inch,
    /// which shows its eighths, and hundredths of the others.
    fn decimals(self) -> u32 {
        match self {
            Unit::Inch => 3,
            _ => 2,
        }
    }
}

/// A length, in layout units, and the unit it's shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Length {
    /// How long, in layout units.
    pub value: i64,
    /// What it's shown in.
    pub unit: Unit,
}

/// The longest length typed that's read: a thousand inches.
const MOST: i64 = 1000 * INCH;

impl Length {
    /// What a person typed, in `unit` unless it names another: "12",
    /// "3,5 cm", "0.5in", "-1 mm", or picas and points, "3p6", "3p" or
    /// "p6". `None` if it isn't a length, or is longer than a thousand
    /// inches.
    pub fn parse(text: &str, unit: Unit) -> Option<Length> {
        let text = text.trim();
        if text.len() > 40 {
            return None;
        }
        let (negative, text) = match text.strip_prefix('-') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, text),
        };
        let (value, unit) = match picas(text) {
            Some(value) => (value, Unit::Pica),
            None => {
                let number_end = text
                    .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
                    .unwrap_or(text.len());
                let (number, suffix) = text.split_at(number_end);
                let suffix = suffix.trim();
                let unit = if suffix.is_empty() {
                    unit
                } else {
                    Unit::from_suffix(suffix)?
                };
                (decimal(number, unit.units())?, unit)
            }
        };
        let value = Some(value).filter(|v| *v <= MOST)?;
        Some(Length {
            value: if negative { -value } else { value },
            unit,
        })
    }
}

/// `number`, digits with a decimal point or comma, of something `per`
/// layout units long, to the nearest layout unit; `None` if it isn't a
/// number, or is far too long. It's all in integers, so it reads the same
/// everywhere.
fn decimal(number: &str, per: i64) -> Option<i64> {
    let mut parts = number.split(['.', ',']);
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some() || (whole.is_empty() && fraction.is_empty()) {
        return None;
    }
    if !whole
        .chars()
        .chain(fraction.chars())
        .all(|c| c.is_ascii_digit())
        || whole.len() + fraction.len() > 20
    {
        return None;
    }
    let mut mantissa: i128 = 0;
    for c in whole.chars().chain(fraction.chars()) {
        mantissa = mantissa * 10 + i128::from(c as u8 - b'0');
    }
    let scale = 10i128.pow(fraction.len() as u32);
    let n = mantissa * i128::from(per);
    i64::try_from((n + scale / 2) / scale).ok()
}

/// Picas and points, as typographers write them: "3p6" is three picas and
/// six points, "3p" three picas, and "p6" six points, in layout units.
/// `None` if `text` isn't written so, as "6pt" isn't.
fn picas(text: &str) -> Option<i64> {
    let (picas, points) = text.split_once(['p', 'P'])?;
    let (picas, points) = (picas.trim(), points.trim());
    if picas.is_empty() && points.is_empty() {
        return None;
    }
    let picas = match picas {
        "" => 0,
        whole if whole.chars().all(|c| c.is_ascii_digit()) => decimal(whole, Unit::Pica.units())?,
        _ => return None,
    };
    let points = match points {
        "" => 0,
        number => decimal(number, Unit::Point.units())?,
    };
    picas.checked_add(points)
}

impl std::fmt::Display for Length {
    /// "3.5 cm", "11 pt", "13.75 cm", "0.125 in", or for picas "8p3.21":
    /// to the hundredth of the unit, the thousandth of an inch, or the
    /// hundredth of a point, with no trailing zeros.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = i128::from(self.value);
        if self.unit == Unit::Pica {
            // Hundredths of a point, and the picas and points they make.
            let point = i128::from(Unit::Point.units());
            let hundredths = (v.abs() * 100 + point / 2) / point;
            let sign = if v < 0 && hundredths > 0 { "-" } else { "" };
            let (picas, points) = (hundredths / 1200, hundredths % 1200);
            return write!(f, "{sign}{picas}p{}", number(points, 2));
        }
        let decimals = self.unit.decimals();
        let scale = 10i128.pow(decimals);
        let units = i128::from(self.unit.units());
        let parts = (v.abs() * scale + units / 2) / units;
        let sign = if v < 0 && parts > 0 { "-" } else { "" };
        write!(
            f,
            "{sign}{} {}",
            number(parts, decimals),
            self.unit.suffix()
        )
    }
}

/// `parts`, a number in units of ten to the minus `decimals`, written with
/// no trailing zeros: 1375 and 2 give "13.75", 1300 and 2 give "13".
fn number(parts: i128, decimals: u32) -> String {
    let scale = 10i128.pow(decimals);
    let (whole, part) = (parts / scale, parts % scale);
    if part == 0 {
        return whole.to_string();
    }
    let digits = format!("{part:0width$}", width = decimals as usize);
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_whole_layout_units() {
        assert_eq!(Unit::Point.units(), 12_700);
        assert_eq!(Unit::Millimetre.units(), 36_000);
        assert_eq!(Unit::Centimetre.units(), 360_000);
        assert_eq!(Unit::Pica.units(), 12 * Unit::Point.units());
        assert_eq!(Unit::Inch.units(), 6 * Unit::Pica.units());
    }

    #[test]
    fn lengths_show_to_the_hundredth() {
        let show = |value, unit| Length { value, unit }.to_string();
        assert_eq!(show(139_700, Unit::Point), "11 pt");
        assert_eq!(show(1_260_000, Unit::Centimetre), "3.5 cm");
        assert_eq!(show(4_950_000, Unit::Centimetre), "13.75 cm");
        assert_eq!(show(0, Unit::Point), "0 pt");
        assert_eq!(show(-540_000, Unit::Centimetre), "-1.5 cm");
        assert_eq!(show(914_400, Unit::Inch), "1 in");
        // Inches to the thousandth, for their eighths.
        assert_eq!(show(114_300, Unit::Inch), "0.125 in");
        assert_eq!(show(900_000, Unit::Inch), "0.984 in");
    }

    #[test]
    fn picas_show_with_their_points() {
        let show = |value| {
            Length {
                value,
                unit: Unit::Pica,
            }
            .to_string()
        };
        let point = Unit::Point.units();
        assert_eq!(show(42 * point), "3p6");
        assert_eq!(show(36 * point), "3p0");
        assert_eq!(show(6 * point), "0p6");
        assert_eq!(show(0), "0p0");
        assert_eq!(show(-18 * point), "-1p6");
        // 3.5 cm is 99.21 points.
        assert_eq!(show(1_260_000), "8p3.21");
        assert_eq!(show(point / 2), "0p0.5");
    }

    #[test]
    fn lengths_are_read_as_typed() {
        let read = |text, unit| Length::parse(text, unit).map(|l| (l.value, l.unit));
        assert_eq!(read("12", Unit::Point), Some((152_400, Unit::Point)));
        assert_eq!(
            read(" 3,5 cm ", Unit::Point),
            Some((1_260_000, Unit::Centimetre))
        );
        assert_eq!(read("0.5in", Unit::Point), Some((457_200, Unit::Inch)));
        assert_eq!(
            read("-1 mm", Unit::Point),
            Some((-36_000, Unit::Millimetre))
        );
        assert_eq!(
            read(".5", Unit::Centimetre),
            Some((180_000, Unit::Centimetre))
        );
        assert_eq!(read("6 PT", Unit::Centimetre), Some((76_200, Unit::Point)));
        for bad in [
            "",
            "-",
            ".",
            "1.2.3",
            "12 furlongs",
            "pt",
            "1e5",
            "99999999999999999999",
        ] {
            assert_eq!(read(bad, Unit::Point), None, "{bad:?}");
        }
        // What's shown reads back as itself.
        for unit in [Unit::Centimetre, Unit::Inch, Unit::Pica] {
            for value in [0, 139_700, 1_260_000, 4_950_000, -540_000, 114_300] {
                let shown = Length { value, unit }.to_string();
                let back = Length::parse(&shown, Unit::Point).unwrap();
                assert_eq!(
                    back.to_string(),
                    shown,
                    "{value} shows as {shown:?} and reads back as {back:?}"
                );
            }
        }
    }

    #[test]
    fn picas_are_read_as_typographers_write_them() {
        let read = |text, unit| Length::parse(text, unit).map(|l| (l.value, l.unit));
        let (pica, point) = (Unit::Pica.units(), Unit::Point.units());
        assert_eq!(
            read("3p6", Unit::Centimetre),
            Some((3 * pica + 6 * point, Unit::Pica))
        );
        assert_eq!(read("3p", Unit::Point), Some((3 * pica, Unit::Pica)));
        assert_eq!(read("p6", Unit::Point), Some((6 * point, Unit::Pica)));
        assert_eq!(read("-1P6", Unit::Point), Some((-18 * point, Unit::Pica)));
        assert_eq!(
            read("8p3.21", Unit::Point),
            Some((8 * pica + 40_767, Unit::Pica))
        );
        assert_eq!(read("2.5 pc", Unit::Point), Some((30 * point, Unit::Pica)));
        // A number alone is in the unit shown.
        assert_eq!(read("4", Unit::Pica), Some((4 * pica, Unit::Pica)));
        // Points are "pt"; "6pt" isn't picas.
        assert_eq!(read("6pt", Unit::Pica), Some((6 * point, Unit::Point)));
        for bad in ["p", "3.5p", "3p6p", "xp6", "p6x"] {
            assert_eq!(read(bad, Unit::Point), None, "{bad:?}");
        }
    }
}
