//! calc's decimal numbers on numbers drawn from every corner of
//! decimal128, the same ones every run: arithmetic never panics, every
//! result is a number decimal128 holds, it prints as text that reads back
//! as itself, and comparing is consistent. Any text reads as a number or
//! as none, without a panic.

use std::cmp::Ordering;

use calc::decimal::{Decimal, Status};

/// Numbers and text drawn by xorshift64*, from a fixed seed.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A number: now and then an infinity or a NaN, otherwise a sign, up
    /// to 40 digits and an exponent from past one end of the range to past
    /// the other, read as text, so rounding, overflow, underflow and
    /// clamping all come up.
    fn decimal(&mut self) -> Decimal {
        let sign = if self.below(2) == 0 { "-" } else { "" };
        let text = match self.below(16) {
            0 => format!("{sign}Infinity"),
            1 => format!("{sign}NaN"),
            2 => format!("{sign}sNaN"),
            _ => {
                let digits: String = (0..1 + self.below(40))
                    .map(|_| char::from(b'0' + self.below(10) as u8))
                    .collect();
                let exponent = self.below(12_400) as i64 - 6_200;
                format!("{sign}{digits}E{exponent}")
            }
        };
        Decimal::parse(&text).unwrap_or_else(|| panic!("{text:?} is a number"))
    }

    /// Text of the characters numbers are made of, in any order.
    fn text(&mut self) -> String {
        const CHARACTERS: &[u8] = b"0123456789.eE+-infatyINFsS ";
        (0..self.below(24))
            .map(|_| char::from(CHARACTERS[self.below(CHARACTERS.len() as u64) as usize]))
            .collect()
    }
}

/// Whether `x` is a number decimal128 holds, and prints as text that
/// reads back as the very same number, exponent and sign included.
fn holds(x: Decimal) -> bool {
    Decimal::from_bits(x.to_bits()).is_some()
        && Decimal::parse(&x.to_string()).map(Decimal::to_bits) == Some(x.to_bits())
}

#[test]
fn arithmetic_never_panics_and_stays_in_range() {
    let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
    for _ in 0..20_000 {
        let (a, b) = (draw.decimal(), draw.decimal());
        assert!(holds(a) && holds(b), "{a} and {b}");
        let mut status = Status::default();
        for (op, result) in [
            ("+", a.add(b, &mut status)),
            ("-", a.sub(b, &mut status)),
            ("*", a.mul(b, &mut status)),
            ("/", a.div(b, &mut status)),
        ] {
            assert!(holds(result), "{a} {op} {b} = {result}");
        }
        assert!(holds(-a), "-({a})");
    }
}

#[test]
fn comparing_is_consistent() {
    let mut draw = Draw(0x2545_f491_4f6c_dd1d);
    for _ in 0..20_000 {
        let (a, b) = (draw.decimal(), draw.decimal());
        let ab = a.compare(b);
        assert_eq!(ab.map(Ordering::reverse), b.compare(a), "{a} and {b}");
        assert_eq!(ab.is_none(), a.is_nan() || b.is_nan(), "{a} and {b}");
        assert_eq!(a == b, ab == Some(Ordering::Equal), "{a} and {b}");
        if !a.is_nan() {
            assert_eq!(a.compare(a), Some(Ordering::Equal), "{a}");
        }
    }
}

#[test]
fn any_text_reads_as_a_number_or_none() {
    let mut draw = Draw(0xd1b5_4a32_d192_ed03);
    for _ in 0..100_000 {
        let text = draw.text();
        if let Some(x) = Decimal::parse(&text) {
            assert!(holds(x), "{text:?} reads as {x}");
        }
    }
    for bad in ["", "1E99999999999999999999999", "--1", "1e+-1", "Infinity1"] {
        if let Some(x) = Decimal::parse(bad) {
            assert!(holds(x), "{bad:?} reads as {x}");
        }
    }
}
