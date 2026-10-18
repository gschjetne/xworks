# The decimal prototype

M1's stage 2 ([docs/ROADMAP.md](../../docs/ROADMAP.md)): the measurements
behind calc's number type. The owner chose 128-bit decimal numbers, to be
confirmed by a prototype against the budgets (docs/ARCHITECTURE.md,
section 7). This compares the candidates.

It's a standalone project, outside the workspace, so candidates under review
don't enter the workspace's dependency graph before a decision admits them.

```sh
./fetch-dectest.sh                        # the test cases, once (not committed)
cargo run --release -- dectest            # decimal128's test cases, per candidate
cargo run --release -- bench              # speed, against f64
cargo run --release -- diff 2000000       # calc against libdecNumber, at random
cargo run --release -- facts              # small cases people notice
```

## Candidates

| Candidate | What it is | License | Builds for WebAssembly |
|---|---|---|---|
| `fastnum` 0.7.5 `D128` | Pure Rust; a 128-bit coefficient with a 16-bit scale, rounding at 38 or 39 digits wherever the binary overflow falls. Not decimal128 | MIT OR Apache-2.0 | yes |
| `dec` 0.4.11 `Decimal128` | Bindings to IBM's libdecNumber, the General Decimal Arithmetic's reference implementation, in C | Apache-2.0, and ICU for the C library (not in `deny.toml`) | no: the C library needs a libc |
| `calc::decimal::Decimal` | Ours: decimal128's arithmetic in pure Rust, 16 bytes, built because neither of the others fits a pure crate | MIT OR Apache-2.0 | yes |

## Results (2026-10-19, AMD Ryzen 7 1800X, Linux, release build)

**Correctness:** the General Decimal Arithmetic test cases for decimal128
(`dq*.decTest`). A case counts when its result matches exactly, digits and
exponent alike. Cases needing other contexts, NaN payloads or raw encodings
are skipped.

| Candidate | add | subtract | multiply | divide | compare | failed |
|---|---|---|---|---|---|---|
| `fastnum` D128 | 405 | 259 | 183 | 148 | 578 | 600 |
| `dec` Decimal128 | 755 | 332 | 330 | 440 | 637 | 0 |
| `calc` Decimal | 755 | 332 | 330 | 440 | 637 | 0 |

`fastnum`'s failures come almost all from rounding at 38 or 39 digits
instead of 34.

**Against libdecNumber at random:** 10,000,000 results (add, subtract,
multiply, divide and compare, operands of 1 to 34 digits across the whole
exponent range, with zeros, infinities and NaNs), compared digit for digit
between `calc` and `dec`: no mismatches.

**Speed**, per operation, on 100,000 amounts with two decimals; and a
million-cell recalculation (each cell the one before times a growth rate
plus an amount, every hundredth divided by twelve, every thousandth a sum of
a thousand amounts), single-threaded:

| Candidate | parse | add | multiply | divide | 1M-cell recalc |
|---|---|---|---|---|---|
| `f64` | 24.9 ns | 0.8 ns | 0.4 ns | 1.1 ns | 13.7 ms |
| `fastnum` D128 | 32.6 ns | 18.0 ns | 10.6 ns | 128.2 ns | 142.3 ms |
| `dec` Decimal128 | 73.2 ns | 51.1 ns | 53.6 ns | 138.2 ns | 242.3 ms |
| `calc` Decimal | 63.9 ns | 30.8 ns | 13.4 ns | 168.9 ns | 160.3 ms |

`dec` and `calc` end the recalculation on the same 34-digit value,
64294561.49762702417961421265919505; `f64` ends on 64294561.49762703.

**Cases people notice:**

| | `f64` | decimal128 (`dec`, `calc`) |
|---|---|---|
| 0.1 + 0.2 | 0.30000000000000004 | 0.3 |
| 10,000 × 0.01, summed | 100.00000000001425 | 100.00 |
| (1 ÷ 3) × 3 | 1.0 | 0.9999999999999999999999999999999999 |
| 10^300 × 10^300 | infinity | 1E+600 |
