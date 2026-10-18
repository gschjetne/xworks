//! Decimal numbers: IEEE 754 decimal128's values and arithmetic, as the
//! General Decimal Arithmetic specifies them, in pure Rust.
//!
//! A [`Decimal`] is a sign, a coefficient of up to 34 decimal digits and an
//! exponent, or an infinity or a NaN. Results are rounded to 34 digits, half
//! to even. Like decimal128, a number keeps its exponent, so `1.50` and
//! `1.5` are equal but print as written; the ideal exponent rules decide a
//! result's.
//!
//! Invariants:
//!
//! - A finite number's coefficient is below 10^34, and its exponent lies
//!   between −6176 and 6111, decimal128's range.
//! - Operations never panic and are exact whenever the exact result fits in
//!   34 digits; otherwise they round once, half to even.
//! - The same operands always give the same result and status, on every
//!   platform.

use std::cmp::Ordering;
use std::fmt;

use crate::u256::{U256, digits_u128};

/// Significant digits a number keeps.
pub const PRECISION: u32 = 34;
/// The largest adjusted exponent (the exponent of the leading digit).
pub const EMAX: i32 = 6144;
/// The smallest adjusted exponent of a normal number.
pub const EMIN: i32 = -6143;
/// The smallest exponent, of the smallest subnormal numbers.
const ETINY: i32 = EMIN - (PRECISION as i32 - 1);
/// The largest exponent a coefficient can have.
const ETOP: i32 = EMAX - (PRECISION as i32 - 1);
/// 10^34: every coefficient is below it.
const LIMIT: u128 = 10u128.pow(PRECISION);

// The encoding inside the `u128`, xWorks' own rather than IEEE's: the
// coefficient in the low 113 bits, a 14-bit field above it (the exponent
// biased by ETINY, or a marker for an infinity or a NaN), and the sign on
// top. 10^34 < 2^113 and decimal128's 12288 exponents < 2^14.
const COEFFICIENT_BITS: u32 = 113;
const COEFFICIENT_MASK: u128 = (1 << COEFFICIENT_BITS) - 1;
const FIELD_MASK: u128 = (1 << 14) - 1;
const SIGN: u128 = 1 << 127;
const FIELD_INFINITY: u128 = (ETOP - ETINY + 1) as u128;
const FIELD_NAN: u128 = FIELD_INFINITY + 1;
const FIELD_SNAN: u128 = FIELD_INFINITY + 2;

/// A decimal number, in 16 bytes.
///
/// Comparisons are by value: `1.50 == 1.5`, `-0 == 0`, and a NaN equals
/// nothing. [`Decimal::to_bits`] tells representations apart.
#[derive(Clone, Copy)]
pub struct Decimal(u128);

/// The conditions an operation raised, as the General Decimal Arithmetic
/// names them. Operations only ever set them, so one `Status` can collect
/// a whole calculation's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The result isn't exact.
    pub inexact: bool,
    /// Digits were dropped, whether or not they were zero.
    pub rounded: bool,
    /// The result was too large and became an infinity.
    pub overflow: bool,
    /// The result was subnormal and inexact.
    pub underflow: bool,
    /// The result is subnormal: smaller than the smallest normal number.
    pub subnormal: bool,
    /// The exponent was changed to fit decimal128's range.
    pub clamped: bool,
    /// A finite number was divided by zero.
    pub division_by_zero: bool,
    /// The operation has no meaningful result, such as ∞ − ∞, 0 ÷ 0 or any
    /// operation on a signaling NaN.
    pub invalid: bool,
}

/// A number taken apart.
enum Parts {
    Finite {
        negative: bool,
        coefficient: u128,
        exponent: i32,
    },
    Infinite {
        negative: bool,
    },
    NaN {
        signaling: bool,
    },
}

impl Decimal {
    /// Zero.
    pub const ZERO: Decimal = Decimal((-ETINY as u128) << COEFFICIENT_BITS);
    /// One.
    pub const ONE: Decimal = Decimal(1 | (-ETINY as u128) << COEFFICIENT_BITS);
    /// A quiet NaN.
    pub const NAN: Decimal = Decimal(FIELD_NAN << COEFFICIENT_BITS);
    /// Positive infinity.
    pub const INFINITY: Decimal = Decimal(FIELD_INFINITY << COEFFICIENT_BITS);

    /// The number's 128 bits, in xWorks' own encoding. Equal bits mean the
    /// same representation, exponent included.
    pub fn to_bits(self) -> u128 {
        self.0
    }

    /// The number with these bits, if they encode one.
    pub fn from_bits(bits: u128) -> Option<Decimal> {
        let field = bits >> COEFFICIENT_BITS & FIELD_MASK;
        let coefficient = bits & COEFFICIENT_MASK;
        let valid = match field {
            f if f < FIELD_INFINITY => coefficient < LIMIT,
            FIELD_INFINITY | FIELD_NAN | FIELD_SNAN => coefficient == 0,
            _ => false,
        };
        valid.then_some(Decimal(bits))
    }

    /// Whether the number is a NaN, quiet or signaling.
    pub fn is_nan(self) -> bool {
        matches!(self.parts(), Parts::NaN { .. })
    }

    /// Whether the number is a signaling NaN.
    pub fn is_signaling(self) -> bool {
        matches!(
            self.parts(),
            Parts::NaN {
                signaling: true,
                ..
            }
        )
    }

    /// Whether the number is an infinity.
    pub fn is_infinite(self) -> bool {
        matches!(self.parts(), Parts::Infinite { .. })
    }

    /// Whether the number is finite.
    pub fn is_finite(self) -> bool {
        matches!(self.parts(), Parts::Finite { .. })
    }

    /// Whether the number is a zero, of either sign.
    pub fn is_zero(self) -> bool {
        matches!(self.parts(), Parts::Finite { coefficient: 0, .. })
    }

    /// Whether the sign is negative; true for −0 and for negative NaNs.
    pub fn is_negative(self) -> bool {
        self.0 & SIGN != 0
    }

    /// Reads a number in the General Decimal Arithmetic's syntax: an
    /// optional sign, digits with an optional decimal point, an optional
    /// exponent (`1.5E-3`), or `Infinity`, `Inf`, `NaN` or `sNaN`, in any
    /// case. More than 34 digits are rounded. `None` if it isn't a number.
    pub fn parse(text: &str) -> Option<Decimal> {
        Decimal::parse_with(text, &mut Status::default())
    }

    /// [`Decimal::parse`], recording what rounding the number raised.
    pub fn parse_with(text: &str, status: &mut Status) -> Option<Decimal> {
        let (negative, rest) = match text.as_bytes().first()? {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let lower = rest.to_ascii_lowercase();
        if lower == "inf" || lower == "infinity" {
            return Some(infinity(negative));
        }
        // NaNs may carry a payload of digits, which isn't kept.
        for (prefix, signaling) in [("snan", true), ("nan", false)] {
            if let Some(payload) = lower.strip_prefix(prefix) {
                return payload
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| nan(negative, signaling));
            }
        }

        let (mantissa, exponent) = match rest.find(['e', 'E']) {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        let mut coefficient: u128 = 0;
        let mut significant = 0;
        let mut seen_digit = false;
        let mut seen_point = false;
        let mut fraction_digits: i64 = 0;
        let mut dropped: i64 = 0;
        let mut sticky = false;
        for b in mantissa.bytes() {
            match b {
                b'0'..=b'9' => {
                    seen_digit = true;
                    if seen_point {
                        fraction_digits += 1;
                    }
                    let digit = (b - b'0') as u128;
                    if significant < PRECISION + 1 {
                        coefficient = coefficient * 10 + digit;
                        if coefficient != 0 {
                            significant += 1;
                        }
                    } else {
                        dropped += 1;
                        sticky |= digit != 0;
                    }
                }
                b'.' if !seen_point => seen_point = true,
                _ => return None,
            }
        }
        if !seen_digit {
            return None;
        }
        let exponent = match exponent {
            Some(text) => parse_exponent(text)?,
            None => 0,
        };
        Some(finish(
            negative,
            U256::from_u128(coefficient),
            exponent - fraction_digits + dropped,
            sticky,
            status,
        ))
    }

    /// `self + rhs`.
    pub fn add(self, rhs: Decimal, status: &mut Status) -> Decimal {
        add_signed(self, rhs, false, status)
    }

    /// `self − rhs`.
    pub fn sub(self, rhs: Decimal, status: &mut Status) -> Decimal {
        add_signed(self, rhs, true, status)
    }

    /// `self × rhs`.
    pub fn mul(self, rhs: Decimal, status: &mut Status) -> Decimal {
        if let Some(nan) = propagate_nan(self, rhs, status) {
            return nan;
        }
        match (self.parts(), rhs.parts()) {
            (
                Parts::Finite {
                    negative: na,
                    coefficient: ca,
                    exponent: ea,
                },
                Parts::Finite {
                    negative: nb,
                    coefficient: cb,
                    exponent: eb,
                },
            ) => {
                let exponent = ea as i64 + eb as i64;
                match ca.checked_mul(cb) {
                    Some(product) => finish_u128(na ^ nb, product, exponent, status),
                    None => finish(na ^ nb, U256::mul_u128(ca, cb), exponent, false, status),
                }
            }
            (Parts::Finite { coefficient: 0, .. }, Parts::Infinite { .. })
            | (Parts::Infinite { .. }, Parts::Finite { coefficient: 0, .. }) => {
                status.invalid = true;
                Decimal::NAN
            }
            _ => infinity(self.is_negative() ^ rhs.is_negative()),
        }
    }

    /// `self ÷ rhs`.
    pub fn div(self, rhs: Decimal, status: &mut Status) -> Decimal {
        if let Some(nan) = propagate_nan(self, rhs, status) {
            return nan;
        }
        let negative = self.is_negative() ^ rhs.is_negative();
        match (self.parts(), rhs.parts()) {
            (Parts::Infinite { .. }, Parts::Infinite { .. }) => {
                status.invalid = true;
                Decimal::NAN
            }
            (Parts::Infinite { .. }, _) => infinity(negative),
            (_, Parts::Infinite { .. }) => {
                status.clamped = true;
                pack(negative, 0, ETINY)
            }
            (
                Parts::Finite {
                    coefficient: ca,
                    exponent: ea,
                    ..
                },
                Parts::Finite {
                    coefficient: cb,
                    exponent: eb,
                    ..
                },
            ) => divide(negative, ca, ea, cb, eb, status),
            _ => unreachable!("NaNs were propagated above"),
        }
    }

    /// Compares by value. `None` when either is a NaN; `-0` equals `0`.
    pub fn compare(self, rhs: Decimal) -> Option<Ordering> {
        let negative_first = |negative: bool| {
            if negative {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        };
        match (self.parts(), rhs.parts()) {
            (Parts::NaN { .. }, _) | (_, Parts::NaN { .. }) => None,
            (Parts::Infinite { negative: na }, Parts::Infinite { negative: nb }) => {
                Some(if na == nb {
                    Ordering::Equal
                } else {
                    negative_first(na)
                })
            }
            (Parts::Infinite { negative }, _) => Some(negative_first(negative)),
            (_, Parts::Infinite { negative }) => Some(negative_first(negative).reverse()),
            (
                Parts::Finite {
                    negative: na,
                    coefficient: ca,
                    exponent: ea,
                },
                Parts::Finite {
                    negative: nb,
                    coefficient: cb,
                    exponent: eb,
                },
            ) => Some(match (ca == 0, cb == 0) {
                (true, true) => Ordering::Equal,
                (true, false) => negative_first(nb).reverse(),
                (false, true) => negative_first(na),
                (false, false) if na != nb => negative_first(na),
                (false, false) => {
                    let magnitude = compare_magnitude(ca, ea, cb, eb);
                    if na { magnitude.reverse() } else { magnitude }
                }
            }),
        }
    }

    fn parts(self) -> Parts {
        let negative = self.is_negative();
        let field = self.0 >> COEFFICIENT_BITS & FIELD_MASK;
        match field {
            FIELD_INFINITY => Parts::Infinite { negative },
            FIELD_NAN => Parts::NaN { signaling: false },
            FIELD_SNAN => Parts::NaN { signaling: true },
            _ => Parts::Finite {
                negative,
                coefficient: self.0 & COEFFICIENT_MASK,
                exponent: field as i32 + ETINY,
            },
        }
    }
}

/// A finite number from parts already within range.
fn pack(negative: bool, coefficient: u128, exponent: i32) -> Decimal {
    debug_assert!(coefficient < LIMIT && (ETINY..=ETOP).contains(&exponent));
    let field = (exponent - ETINY) as u128;
    Decimal(coefficient | field << COEFFICIENT_BITS | if negative { SIGN } else { 0 })
}

fn infinity(negative: bool) -> Decimal {
    Decimal(FIELD_INFINITY << COEFFICIENT_BITS | if negative { SIGN } else { 0 })
}

fn nan(negative: bool, signaling: bool) -> Decimal {
    let field = if signaling { FIELD_SNAN } else { FIELD_NAN };
    Decimal(field << COEFFICIENT_BITS | if negative { SIGN } else { 0 })
}

/// The NaN an operation on `a` and `b` gives, if either is one: the first
/// signaling NaN, made quiet, else the first NaN.
fn propagate_nan(a: Decimal, b: Decimal, status: &mut Status) -> Option<Decimal> {
    for x in [a, b] {
        if x.is_signaling() {
            status.invalid = true;
            return Some(nan(x.is_negative(), false));
        }
    }
    [a, b].into_iter().find(|x| x.is_nan())
}

/// Reads an exponent's digits, with an optional sign. Exponents far beyond
/// decimal128's range are capped: they overflow or underflow just the same.
fn parse_exponent(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = (value * 10 + (b - b'0') as i64).min(1_000_000_000);
    }
    Some(if negative { -value } else { value })
}

fn add_signed(a: Decimal, b: Decimal, negate_b: bool, status: &mut Status) -> Decimal {
    if let Some(nan) = propagate_nan(a, b, status) {
        return nan;
    }
    match (a.parts(), b.parts()) {
        (Parts::Infinite { negative: na }, Parts::Infinite { negative: nb }) => {
            if na == nb ^ negate_b {
                infinity(na)
            } else {
                status.invalid = true;
                Decimal::NAN
            }
        }
        (Parts::Infinite { negative }, _) => infinity(negative),
        (_, Parts::Infinite { negative }) => infinity(negative ^ negate_b),
        (
            Parts::Finite {
                negative: na,
                coefficient: ca,
                exponent: ea,
            },
            Parts::Finite {
                negative: nb,
                coefficient: cb,
                exponent: eb,
            },
        ) => add_finite((na, ca, ea), (nb ^ negate_b, cb, eb), status),
        _ => unreachable!("NaNs were propagated above"),
    }
}

/// The sum of two finite numbers, each a sign, a coefficient and an
/// exponent.
fn add_finite(a: (bool, u128, i32), b: (bool, u128, i32), status: &mut Status) -> Decimal {
    let ideal = a.2.min(b.2);
    if a.1 == 0 || b.1 == 0 {
        if a.1 == 0 && b.1 == 0 {
            return zero(a.0 && b.0, ideal as i64, status);
        }
        // x + 0: x, moved towards the ideal exponent as far as its digits
        // allow.
        let (negative, coefficient, exponent) = if a.1 == 0 { b } else { a };
        let room = PRECISION - digits_u128(coefficient);
        let shift = ((exponent - ideal) as u32).min(room);
        let coefficient = coefficient * 10u128.pow(shift);
        return finish_u128(
            negative,
            coefficient,
            exponent as i64 - shift as i64,
            status,
        );
    }

    // Order them so that a has the larger exponent.
    let ((na, ca, ea), (nb, cb, eb)) = if a.2 >= b.2 { (a, b) } else { (b, a) };
    let gap = (ea + digits_u128(ca) as i32) - (eb + digits_u128(cb) as i32);
    if gap > PRECISION as i32 + 2 {
        // b lies wholly below a's guard digit, so it only decides rounding:
        // a, scaled to 36 digits, nudged towards b.
        let k = PRECISION + 2 - digits_u128(ca);
        let scaled = ca * 10u128.pow(k);
        let coefficient = if na == nb { scaled } else { scaled - 1 };
        return finish(
            na,
            U256::from_u128(coefficient),
            ea as i64 - k as i64,
            true,
            status,
        );
    }

    // Exact: align a to b's exponent. The gap bounds the shift, so the
    // aligned coefficient has at most 70 digits.
    let shift = (ea - eb) as u32;
    if let Some(aligned) = 10u128.checked_pow(shift).and_then(|p| ca.checked_mul(p)) {
        if na == nb {
            if let Some(sum) = aligned.checked_add(cb) {
                return finish_u128(na, sum, eb as i64, status);
            }
        } else {
            return match aligned.cmp(&cb) {
                Ordering::Greater => finish_u128(na, aligned - cb, eb as i64, status),
                Ordering::Less => finish_u128(nb, cb - aligned, eb as i64, status),
                Ordering::Equal => zero(false, eb as i64, status),
            };
        }
    }
    let aligned = U256::from_u128(ca)
        .mul_pow10(shift)
        .expect("the gap bounds the aligned coefficient to 70 digits");
    let cb = U256::from_u128(cb);
    let (negative, coefficient) = if na == nb {
        (na, aligned.add(cb))
    } else {
        match aligned.cmp(&cb) {
            Ordering::Greater => (na, aligned.sub(cb)),
            Ordering::Less => (nb, cb.sub(aligned)),
            Ordering::Equal => return zero(false, eb as i64, status),
        }
    };
    finish(negative, coefficient, eb as i64, false, status)
}

/// The quotient of two finite numbers' coefficients and exponents.
fn divide(negative: bool, ca: u128, ea: i32, cb: u128, eb: i32, status: &mut Status) -> Decimal {
    let ideal = ea as i64 - eb as i64;
    if cb == 0 {
        if ca == 0 {
            status.invalid = true;
            return Decimal::NAN;
        }
        status.division_by_zero = true;
        return infinity(negative);
    }
    if ca == 0 {
        return zero(negative, ideal, status);
    }
    // Long division, four digits at a time, until the quotient has a digit
    // beyond the precision or comes out exact. The remainder stays below
    // cb < 10^34, so ×10^4 fits in a u128, and the quotient stays below
    // 10^38.
    let mut quotient = ca / cb;
    let mut remainder = ca % cb;
    let mut exponent = ideal;
    while remainder != 0 && (quotient == 0 || digits_u128(quotient) <= PRECISION) {
        remainder *= 10_000;
        quotient = quotient * 10_000 + remainder / cb;
        remainder %= cb;
        exponent -= 4;
    }
    if remainder == 0 {
        // Exact: back towards the ideal exponent.
        while exponent < ideal && quotient.is_multiple_of(10) {
            quotient /= 10;
            exponent += 1;
        }
        finish_u128(negative, quotient, exponent, status)
    } else {
        finish(negative, U256::from_u128(quotient), exponent, true, status)
    }
}

/// Compares the magnitudes of two nonzero finite numbers.
fn compare_magnitude(ca: u128, ea: i32, cb: u128, eb: i32) -> Ordering {
    let adjusted_a = ea + digits_u128(ca) as i32;
    let adjusted_b = eb + digits_u128(cb) as i32;
    if adjusted_a != adjusted_b {
        return adjusted_a.cmp(&adjusted_b);
    }
    // Same leading digit position, so the exponents differ by less than the
    // precision.
    if ea >= eb {
        let aligned = U256::from_u128(ca)
            .mul_pow10((ea - eb) as u32)
            .expect("within 68 digits");
        aligned.cmp(&U256::from_u128(cb))
    } else {
        let aligned = U256::from_u128(cb)
            .mul_pow10((eb - ea) as u32)
            .expect("within 68 digits");
        U256::from_u128(ca).cmp(&aligned)
    }
}

/// A zero with its exponent brought into range.
fn zero(negative: bool, exponent: i64, status: &mut Status) -> Decimal {
    let clamped = exponent.clamp(ETINY as i64, ETOP as i64);
    if clamped != exponent {
        status.clamped = true;
    }
    pack(negative, 0, clamped as i32)
}

/// [`finish`] for an exact result in a `u128`, with a fast path for the
/// common case of a normal number that needs no rounding.
fn finish_u128(negative: bool, coefficient: u128, exponent: i64, status: &mut Status) -> Decimal {
    if coefficient != 0
        && coefficient < LIMIT
        && (ETINY as i64..=ETOP as i64).contains(&exponent)
        && exponent + digits_u128(coefficient) as i64 > EMIN as i64
    {
        return pack(negative, coefficient, exponent as i32);
    }
    finish(
        negative,
        U256::from_u128(coefficient),
        exponent,
        false,
        status,
    )
}

/// The number `coefficient × 10^exponent`, plus a sliver below its last
/// digit when `sticky` is set, rounded to 34 digits half to even and brought
/// into decimal128's range. When `sticky` is set, the coefficient must have
/// more than 34 digits, so a guard digit decides ties.
fn finish(
    negative: bool,
    coefficient: U256,
    exponent: i64,
    sticky: bool,
    status: &mut Status,
) -> Decimal {
    if coefficient.is_zero() && !sticky {
        return zero(negative, exponent, status);
    }
    let digits = coefficient.digits() as i64;
    let subnormal = exponent + digits - 1 < EMIN as i64;
    let drop = (digits - PRECISION as i64)
        .max(ETINY as i64 - exponent)
        .max(0);

    let (mut kept, mut exponent) = if drop == 0 {
        debug_assert!(!sticky, "a sticky result comes with a guard digit");
        (coefficient.to_u128().expect("at most 34 digits"), exponent)
    } else {
        let (kept, round_digit, rest) = shift_right(coefficient, drop);
        let rest = rest || sticky;
        let inexact = round_digit != 0 || rest;
        let up = round_digit > 5 || (round_digit == 5 && (rest || kept % 2 == 1));
        status.rounded = true;
        status.inexact |= inexact;
        if subnormal && inexact {
            status.underflow = true;
            status.subnormal = true;
        }
        let mut kept = kept + up as u128;
        let mut exponent = exponent + drop;
        if kept == LIMIT {
            kept /= 10;
            exponent += 1;
        }
        if kept == 0 {
            status.clamped = true;
            return pack(negative, 0, exponent as i32);
        }
        (kept, exponent)
    };

    if exponent + digits_u128(kept) as i64 - 1 > EMAX as i64 {
        status.overflow = true;
        status.inexact = true;
        status.rounded = true;
        return infinity(negative);
    }
    if exponent > ETOP as i64 {
        // Fold down: pad the coefficient with zeros to bring the exponent
        // into range. The adjusted exponent's bound leaves room for them.
        kept *= 10u128.pow((exponent - ETOP as i64) as u32);
        exponent = ETOP as i64;
        status.clamped = true;
    }
    if subnormal {
        status.subnormal = true;
    }
    pack(negative, kept, exponent as i32)
}

/// Drops `drop` digits from the right of `coefficient`. Returns what's
/// kept (at most 34 digits), the first dropped digit and whether any digit
/// after it is nonzero.
fn shift_right(coefficient: U256, drop: i64) -> (u128, u8, bool) {
    let digits = coefficient.digits() as i64;
    if drop > digits {
        return (0, 0, !coefficient.is_zero());
    }
    let (above, rest) = coefficient.div_pow10((drop - 1) as u32);
    let (kept, round_digit) = above.div_rem_u64(10);
    (
        kept.to_u128()
            .expect("a rounded coefficient has at most 34 digits"),
        round_digit as u8,
        rest,
    )
}

impl fmt::Display for Decimal {
    /// Scientific notation, as the General Decimal Arithmetic's
    /// to-scientific-string writes it: `123.45`, `0.000012`, `1.2E+7`,
    /// `0E-7`, `-Infinity`, `NaN`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.is_negative() { "-" } else { "" };
        let (coefficient, exponent) = match self.parts() {
            Parts::Infinite { .. } => return write!(f, "{sign}Infinity"),
            Parts::NaN {
                signaling: false, ..
            } => return write!(f, "{sign}NaN"),
            Parts::NaN {
                signaling: true, ..
            } => return write!(f, "{sign}sNaN"),
            Parts::Finite {
                coefficient,
                exponent,
                ..
            } => (coefficient, exponent),
        };
        let digits = coefficient.to_string();
        let adjusted = exponent + digits.len() as i32 - 1;
        if exponent <= 0 && adjusted >= -6 {
            let point = digits.len() as i32 + exponent;
            if exponent == 0 {
                write!(f, "{sign}{digits}")
            } else if point <= 0 {
                let zeros = "0".repeat((-point) as usize);
                write!(f, "{sign}0.{zeros}{digits}")
            } else {
                let (whole, fraction) = digits.split_at(point as usize);
                write!(f, "{sign}{whole}.{fraction}")
            }
        } else {
            let (first, rest) = digits.split_at(1);
            let point = if rest.is_empty() { "" } else { "." };
            let exponent_sign = if adjusted >= 0 { "+" } else { "" };
            write!(f, "{sign}{first}{point}{rest}E{exponent_sign}{adjusted}")
        }
    }
}

impl fmt::Debug for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Decimal({self})")
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Decimal) -> bool {
        self.compare(*other) == Some(Ordering::Equal)
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Decimal) -> Option<Ordering> {
        self.compare(*other)
    }
}

impl std::ops::Neg for Decimal {
    type Output = Decimal;
    fn neg(self) -> Decimal {
        Decimal(self.0 ^ SIGN)
    }
}

impl From<i64> for Decimal {
    fn from(n: i64) -> Decimal {
        pack(n < 0, n.unsigned_abs() as u128, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).unwrap_or_else(|| panic!("{text:?} parses"))
    }

    fn s(x: Decimal) -> String {
        x.to_string()
    }

    #[test]
    fn decimal_fractions_are_exact() {
        let mut st = Status::default();
        assert_eq!(s(d("0.1").add(d("0.2"), &mut st)), "0.3");
        assert!(!st.inexact);
        let mut sum = Decimal::ZERO;
        for _ in 0..10_000 {
            sum = sum.add(d("0.01"), &mut st);
        }
        assert_eq!(s(sum), "100.00");
    }

    #[test]
    fn a_third_rounds_to_34_digits_and_back_short_of_one() {
        let mut st = Status::default();
        let third = Decimal::ONE.div(d("3"), &mut st);
        assert_eq!(s(third), format!("0.{}", "3".repeat(34)));
        assert!(st.inexact);
        assert_eq!(
            s(third.mul(d("3"), &mut st)),
            format!("0.{}", "9".repeat(34))
        );
    }

    #[test]
    fn ties_round_to_even() {
        let mut st = Status::default();
        let big = d(&"1".repeat(34));
        assert_eq!(
            s(big.add(d("0.5"), &mut st)),
            "1111111111111111111111111111111112"
        );
        let even = d(&format!("{}2", "1".repeat(33)));
        assert_eq!(
            s(even.add(d("0.5"), &mut st)),
            "1111111111111111111111111111111112"
        );
    }

    #[test]
    fn exponents_are_kept_and_compared_by_value() {
        assert_eq!(s(d("1.50")), "1.50");
        assert_eq!(d("1.50"), d("1.5"));
        assert_ne!(d("1.50").to_bits(), d("1.5").to_bits());
        let mut st = Status::default();
        assert_eq!(s(d("1.20").mul(d("3"), &mut st)), "3.60");
        assert_eq!(s(d("6.0").div(d("2"), &mut st)), "3.0");
    }

    #[test]
    fn notation_follows_the_specification() {
        for (input, output) in [
            ("123.45", "123.45"),
            ("0.000012", "0.000012"),
            ("0.0000001", "1E-7"),
            ("12E6", "1.2E+7"),
            ("-0", "-0"),
            ("0E-7", "0E-7"),
            ("0E3", "0E+3"),
            ("inf", "Infinity"),
            ("-INFINITY", "-Infinity"),
            ("nan", "NaN"),
            ("-sNaN", "-sNaN"),
        ] {
            assert_eq!(s(d(input)), output, "{input}");
        }
        for bad in ["", "-", ".", "1.2.3", "1E", "1E+", "abc", "1 2", "NaNx"] {
            assert!(Decimal::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn edges_of_the_range() {
        let mut st = Status::default();
        let huge = d("9.999999999999999999999999999999999E+6144");
        assert!(huge.mul(d("10"), &mut st).is_infinite());
        assert!(st.overflow);
        assert_eq!(s(d("1E+6144")), "1.000000000000000000000000000000000E+6144");
        let mut st = Status::default();
        assert_eq!(s(d("1E-6176").div(d("10"), &mut st)), "0E-6176");
        assert!(st.underflow && st.clamped);
    }

    #[test]
    fn special_values() {
        let mut st = Status::default();
        assert!(Decimal::ONE.div(Decimal::ZERO, &mut st).is_infinite());
        assert!(st.division_by_zero);
        let mut st = Status::default();
        assert!(Decimal::ZERO.div(Decimal::ZERO, &mut st).is_nan());
        assert!(st.invalid);
        assert!(Decimal::INFINITY.sub(Decimal::INFINITY, &mut st).is_nan());
        assert_eq!(Decimal::NAN.compare(Decimal::ONE), None);
        assert_eq!(s(d("-1").div(Decimal::INFINITY, &mut st)), "-0E-6176");
    }

    #[test]
    fn bits_round_trip_and_reject_what_no_number_has() {
        for text in [
            "0",
            "-1.5",
            "1E+6144",
            "1E-6176",
            "-Infinity",
            "NaN",
            "sNaN",
        ] {
            let x = d(text);
            assert_eq!(Decimal::from_bits(x.to_bits()).map(s), Some(s(x)));
        }
        assert!(Decimal::from_bits(LIMIT).is_none());
        assert!(Decimal::from_bits(FIELD_MASK << COEFFICIENT_BITS).is_none());
    }
}
