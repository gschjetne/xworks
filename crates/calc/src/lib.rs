//! Values and formulas: typed values, 128-bit decimal numbers, the formula
//! language, the function library and recalculation (docs/ARCHITECTURE.md,
//! section 7). So far it has M1's stage 2: [`decimal`], the number type,
//! IEEE 754 decimal128's arithmetic in pure Rust, as the General Decimal
//! Arithmetic specifies it.
//!
//! - [`decimal::Decimal`]: a sign, a coefficient of up to 34 decimal
//!   digits and an exponent, or an infinity or a NaN, in 16 bytes; added,
//!   subtracted, multiplied, divided, compared, read from text and
//!   written as text.
//! - [`decimal::Status`]: the conditions an operation raised, which
//!   operations only ever set, so one collects a whole calculation's.
//!
//! # What a caller may rely on
//!
//! - **Pure and the same everywhere.** No I/O, threads, clocks or
//!   environment (Clippy's `crates/clippy.toml`), and it builds for
//!   WebAssembly (CI). The arithmetic is in integers, never binary floating
//!   point, so the same operands give the same result and status on every
//!   platform.
//! - **decimal128's arithmetic, exactly.** Results are exact whenever they
//!   fit in 34 digits, and otherwise rounded once, half to even:
//!   `0.1 + 0.2` is `0.3`. Every case of the General Decimal Arithmetic's
//!   decimal128 tests for addition, subtraction, multiplication, division
//!   and comparison matches, digits and exponent alike, but those needing
//!   what calc doesn't keep (below) (`tests/dectest.rs`,
//!   `decimal128_test_cases_pass`, which CI runs on the fetched cases;
//!   `decimal`'s unit tests `decimal_fractions_are_exact` and
//!   `ties_round_to_even`).
//! - **Always in range, never a panic.** Every result, of any operands,
//!   is a number decimal128 holds: a coefficient below 10^34 and an
//!   exponent from −6176 to 6111, or an infinity or a NaN, with overflow,
//!   underflow and clamping in its [`decimal::Status`]
//!   (`tests/arithmetic.rs`, `arithmetic_never_panics_and_stays_in_range`;
//!   `decimal`'s unit tests `edges_of_the_range` and `special_values`).
//! - **Equality is exact,** by value, with no tolerance: `1.50 == 1.5`,
//!   `-0 == 0`, `(1/3)*3` isn't 1, and a NaN equals nothing. Comparing is
//!   consistent both ways round, so it sorts (`tests/arithmetic.rs`,
//!   `comparing_is_consistent`; `decimal`'s unit tests
//!   `exponents_are_kept_and_compared_by_value` and
//!   `a_third_rounds_to_34_digits_and_back_short_of_one`).
//! - **A number keeps its exponent,** so `1.50` prints as written, and the
//!   ideal exponent rules decide a result's.
//! - **Text:** numbers print in the General Decimal Arithmetic's
//!   scientific notation and read back as the very same number, exponent
//!   and sign included; [`decimal::Decimal::parse`] reads any text as a
//!   number or as none, never panicking, rounding what has more than 34
//!   digits (`tests/arithmetic.rs`, `any_text_reads_as_a_number_or_none`;
//!   `decimal`'s unit test `notation_follows_the_specification`).
//! - **Bits:** [`decimal::Decimal::from_bits`] refuses bits that encode no
//!   number, and gives back any number's own (`decimal`'s unit test
//!   `bits_round_trip_and_reject_what_no_number_has`).
//!
//! # What it doesn't promise
//!
//! - Values, formulas, the function library and recalculation: they come
//!   in M3, with the grid.
//! - Any context but decimal128's defaults (34 digits, half to even), or
//!   NaN payloads, which aren't kept.
//! - IEEE 754's interchange encodings: [`decimal::Decimal::to_bits`] is
//!   xWorks' own, and may change until files store numbers.
//! - Speed: about 30 ns to add and 13 ns to multiply, where binary takes
//!   about one (docs/ARCHITECTURE.md, section 7).

pub mod decimal;
mod u256;
