//! The candidates for calc's number type, behind one interface, and `f64`
//! as the baseline.

use std::cmp::Ordering;

/// A number type under evaluation. Each carries the context its arithmetic
/// needs (rounding, precision), made once and reused, as calc would.
pub trait Number: Copy {
    /// What the arithmetic needs besides its operands.
    type Ctx;
    /// The candidate's name, in reports.
    const NAME: &'static str;
    /// A context with IEEE 754 decimal128's defaults: 34 digits, rounding
    /// half to even, no traps.
    fn ctx() -> Self::Ctx;
    /// Parses a number in the General Decimal Arithmetic's syntax.
    fn parse(ctx: &mut Self::Ctx, text: &str) -> Option<Self>;
    /// The number in scientific notation, as the test cases write results.
    fn text(self) -> String;
    /// `a + b`.
    fn add(ctx: &mut Self::Ctx, a: Self, b: Self) -> Self;
    /// `a - b`.
    fn sub(ctx: &mut Self::Ctx, a: Self, b: Self) -> Self;
    /// `a × b`.
    fn mul(ctx: &mut Self::Ctx, a: Self, b: Self) -> Self;
    /// `a ÷ b`.
    fn div(ctx: &mut Self::Ctx, a: Self, b: Self) -> Self;
    /// Compares by value; `None` when either is NaN.
    fn compare(ctx: &mut Self::Ctx, a: Self, b: Self) -> Option<Ordering>;
    /// Whether the number is a NaN.
    fn is_nan(self) -> bool;
    /// Whether the number's sign is negative, zeros included.
    fn is_negative(self) -> bool;
}

impl Number for f64 {
    type Ctx = ();
    const NAME: &'static str = "f64";
    fn ctx() {}
    fn parse(_: &mut (), text: &str) -> Option<f64> {
        text.parse().ok()
    }
    fn text(self) -> String {
        format!("{self:?}")
    }
    fn add(_: &mut (), a: f64, b: f64) -> f64 {
        a + b
    }
    fn sub(_: &mut (), a: f64, b: f64) -> f64 {
        a - b
    }
    fn mul(_: &mut (), a: f64, b: f64) -> f64 {
        a * b
    }
    fn div(_: &mut (), a: f64, b: f64) -> f64 {
        a / b
    }
    fn compare(_: &mut (), a: f64, b: f64) -> Option<Ordering> {
        a.partial_cmp(&b)
    }
    fn is_nan(self) -> bool {
        f64::is_nan(self)
    }
    fn is_negative(self) -> bool {
        self.is_sign_negative()
    }
}

#[cfg(feature = "fastnum")]
mod fast {
    use std::cmp::Ordering;

    use fastnum::D128;
    use fastnum::decimal::Context;

    impl super::Number for D128 {
        type Ctx = Context;
        const NAME: &'static str = "fastnum D128";
        fn ctx() -> Context {
            Context::default().without_traps()
        }
        fn parse(ctx: &mut Context, text: &str) -> Option<D128> {
            D128::from_str(text, *ctx).ok()
        }
        fn text(self) -> String {
            self.to_string()
        }
        fn add(_: &mut Context, a: D128, b: D128) -> D128 {
            a + b
        }
        fn sub(_: &mut Context, a: D128, b: D128) -> D128 {
            a - b
        }
        fn mul(_: &mut Context, a: D128, b: D128) -> D128 {
            a * b
        }
        fn div(_: &mut Context, a: D128, b: D128) -> D128 {
            a / b
        }
        fn compare(_: &mut Context, a: D128, b: D128) -> Option<Ordering> {
            a.partial_cmp(&b)
        }
        fn is_nan(self) -> bool {
            D128::is_nan(&self)
        }
        fn is_negative(self) -> bool {
            D128::is_sign_negative(&self)
        }
    }
}

#[cfg(feature = "dec")]
mod decnumber {
    use std::cmp::Ordering;

    use dec::{Context, Decimal128};

    impl super::Number for Decimal128 {
        type Ctx = Context<Decimal128>;
        const NAME: &'static str = "dec Decimal128";
        fn ctx() -> Context<Decimal128> {
            Context::default()
        }
        fn parse(ctx: &mut Context<Decimal128>, text: &str) -> Option<Decimal128> {
            ctx.parse(text).ok()
        }
        fn text(self) -> String {
            self.to_string()
        }
        fn add(ctx: &mut Context<Decimal128>, a: Decimal128, b: Decimal128) -> Decimal128 {
            ctx.add(a, b)
        }
        fn sub(ctx: &mut Context<Decimal128>, a: Decimal128, b: Decimal128) -> Decimal128 {
            ctx.sub(a, b)
        }
        fn mul(ctx: &mut Context<Decimal128>, a: Decimal128, b: Decimal128) -> Decimal128 {
            ctx.mul(a, b)
        }
        fn div(ctx: &mut Context<Decimal128>, a: Decimal128, b: Decimal128) -> Decimal128 {
            ctx.div(a, b)
        }
        fn compare(
            ctx: &mut Context<Decimal128>,
            a: Decimal128,
            b: Decimal128,
        ) -> Option<Ordering> {
            ctx.partial_cmp(a, b)
        }
        fn is_nan(self) -> bool {
            Decimal128::is_nan(&self)
        }
        fn is_negative(self) -> bool {
            Decimal128::is_negative(&self)
        }
    }
}

#[cfg(feature = "calc")]
mod ours {
    use std::cmp::Ordering;

    use calc::decimal::{Decimal, Status};

    impl super::Number for Decimal {
        type Ctx = Status;
        const NAME: &'static str = "calc Decimal";
        fn ctx() -> Status {
            Status::default()
        }
        fn parse(status: &mut Status, text: &str) -> Option<Decimal> {
            Decimal::parse_with(text, status)
        }
        fn text(self) -> String {
            self.to_string()
        }
        fn add(status: &mut Status, a: Decimal, b: Decimal) -> Decimal {
            a.add(b, status)
        }
        fn sub(status: &mut Status, a: Decimal, b: Decimal) -> Decimal {
            a.sub(b, status)
        }
        fn mul(status: &mut Status, a: Decimal, b: Decimal) -> Decimal {
            a.mul(b, status)
        }
        fn div(status: &mut Status, a: Decimal, b: Decimal) -> Decimal {
            a.div(b, status)
        }
        fn compare(_: &mut Status, a: Decimal, b: Decimal) -> Option<Ordering> {
            a.compare(b)
        }
        fn is_nan(self) -> bool {
            Decimal::is_nan(self)
        }
        fn is_negative(self) -> bool {
            Decimal::is_negative(self)
        }
    }
}
