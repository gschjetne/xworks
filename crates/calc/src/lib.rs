//! Values and formulas: typed values, 128-bit decimal numbers, the formula
//! language, the function library and recalculation (docs/ARCHITECTURE.md,
//! section 7).
//!
//! So far, M1's stage 2: [`decimal`], the number type, IEEE 754
//! decimal128's arithmetic in pure Rust.

pub mod decimal;
mod u256;
