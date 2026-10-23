//! Lengths as people read and type them: a number and a unit.

/// A unit lengths are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    /// Typographic points, 1/72 inch: type sizes and spacing.
    Point,
    /// Centimetres: the ruler and tab stops.
    Centimetre,
    /// Millimetres: margins.
    Millimetre,
    /// Inches.
    Inch,
}

/// Layout units to the inch (docs/ARCHITECTURE.md, section 9).
const INCH: i64 = 914_400;

impl Unit {
    /// How many layout units it is.
    pub fn units(self) -> i64 {
        match self {
            Unit::Point => INCH / 72,
            Unit::Centimetre => INCH * 10 / 254 * 10,
            Unit::Millimetre => INCH * 10 / 254,
            Unit::Inch => INCH,
        }
    }

    /// What follows a number in it: "pt", "cm", "mm" or "in".
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Point => "pt",
            Unit::Centimetre => "cm",
            Unit::Millimetre => "mm",
            Unit::Inch => "in",
        }
    }

    fn from_suffix(suffix: &str) -> Option<Unit> {
        Some(match suffix.to_lowercase().as_str() {
            "pt" | "p" => Unit::Point,
            "cm" => Unit::Centimetre,
            "mm" => Unit::Millimetre,
            "in" | "\"" | "″" => Unit::Inch,
            _ => return None,
        })
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
    /// "3,5 cm", "0.5in", "-1 mm". `None` if it isn't a length, or is
    /// longer than a thousand inches.
    pub fn parse(text: &str, unit: Unit) -> Option<Length> {
        let text = text.trim();
        if text.len() > 40 {
            return None;
        }
        let number_end = text
            .char_indices()
            .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || c == ',' || (i == 0 && c == '-')))
            .map_or(text.len(), |(i, _)| i);
        let (number, suffix) = text.split_at(number_end);
        let suffix = suffix.trim();
        let unit = if suffix.is_empty() {
            unit
        } else {
            Unit::from_suffix(suffix)?
        };
        let (negative, digits) = match number.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, number),
        };
        let mut parts = digits.split(['.', ',']);
        let whole = parts.next()?;
        let fraction = parts.next().unwrap_or("");
        if parts.next().is_some() || (whole.is_empty() && fraction.is_empty()) {
            return None;
        }
        if !whole
            .chars()
            .chain(fraction.chars())
            .all(|c| c.is_ascii_digit())
        {
            return None;
        }
        // The number as hundredths of the unit, and then the nearest layout
        // unit, all in integers so it reads the same everywhere.
        let mut mantissa: i128 = 0;
        for c in whole.chars().chain(fraction.chars()) {
            mantissa = mantissa * 10 + i128::from(c as u8 - b'0');
        }
        let scale = 10i128.pow(fraction.len() as u32);
        let n = mantissa * i128::from(unit.units());
        let value = (n + scale / 2) / scale;
        let value = i64::try_from(value).ok().filter(|v| *v <= MOST)?;
        Some(Length {
            value: if negative { -value } else { value },
            unit,
        })
    }
}

impl std::fmt::Display for Length {
    /// "3.5 cm", "11 pt", "13.75 cm": to the hundredth of the unit, with
    /// no trailing zeros.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let units = i128::from(self.unit.units());
        let v = i128::from(self.value);
        let hundredths = (v.abs() * 100 + units / 2) / units;
        let sign = if v < 0 && hundredths > 0 { "-" } else { "" };
        let (whole, part) = (hundredths / 100, hundredths % 100);
        let number = match part {
            0 => format!("{whole}"),
            p if p % 10 == 0 => format!("{whole}.{}", p / 10),
            p => format!("{whole}.{p:02}"),
        };
        write!(f, "{sign}{number} {}", self.unit.suffix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_whole_layout_units() {
        assert_eq!(Unit::Point.units(), 12_700);
        assert_eq!(Unit::Millimetre.units(), 36_000);
        assert_eq!(Unit::Centimetre.units(), 360_000);
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
        for value in [0, 139_700, 1_260_000, 4_950_000, -540_000] {
            let shown = Length {
                value,
                unit: Unit::Centimetre,
            }
            .to_string();
            let back = Length::parse(&shown, Unit::Point).unwrap();
            assert_eq!(
                back.to_string(),
                shown,
                "{value} shows as {shown:?} and reads back as {back:?}"
            );
        }
    }
}
