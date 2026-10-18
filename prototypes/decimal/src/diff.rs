//! Differential testing: random operands across decimal128's whole range,
//! every operation's result compared digit for digit between calc's Decimal
//! and libdecNumber, the General Decimal Arithmetic's reference
//! implementation.

#[cfg(all(feature = "calc", feature = "dec"))]
pub fn run(count: usize) {
    use calc::decimal::{Decimal, Status};
    use dec::{Context, Decimal128};

    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // Operands of 1 to 34 digits, with exponents mostly near zero (where
    // sheets live) and sometimes anywhere in range, including the edges,
    // and now and then a zero, an infinity or a NaN.
    let mut operand = move || -> String {
        let r = next();
        match r % 100 {
            0 => return "0".into(),
            1 => return "-Infinity".into(),
            2 => return "NaN".into(),
            _ => {}
        }
        let digits = 1 + (next() % 34) as usize;
        let mut text: String = (0..digits)
            .map(|_| char::from(b'0' + (next() % 10) as u8))
            .collect();
        if text.starts_with('0') && digits > 1 {
            text.replace_range(0..1, "7");
        }
        let exponent: i64 = match next() % 10 {
            0..=6 => (next() % 41) as i64 - 20,
            7 => (next() % 12_300) as i64 - 6_190,
            8 => 6_111 - (next() % 40) as i64,
            _ => -6_176 + (next() % 40) as i64,
        };
        let sign = if next() % 2 == 0 { "-" } else { "" };
        format!("{sign}{text}E{exponent}")
    };

    let mut ctx = Context::<Decimal128>::default();
    let mut mismatches = 0;
    let mut checked = 0;
    for _ in 0..count {
        let (a, b) = (operand(), operand());
        let (Some(xa), Some(xb)) = (Decimal::parse(&a), Decimal::parse(&b)) else {
            println!("calc can't parse {a} or {b}");
            mismatches += 1;
            continue;
        };
        let (Ok(ya), Ok(yb)) = (ctx.parse(a.as_str()), ctx.parse(b.as_str())) else {
            continue;
        };
        let mut st = Status::default();
        let results = [
            (
                "+",
                xa.add(xb, &mut st).to_string(),
                ctx.add(ya, yb).to_string(),
            ),
            (
                "-",
                xa.sub(xb, &mut st).to_string(),
                ctx.sub(ya, yb).to_string(),
            ),
            (
                "*",
                xa.mul(xb, &mut st).to_string(),
                ctx.mul(ya, yb).to_string(),
            ),
            (
                "/",
                xa.div(xb, &mut st).to_string(),
                ctx.div(ya, yb).to_string(),
            ),
            (
                "cmp",
                format!("{:?}", xa.compare(xb)),
                format!("{:?}", ctx.partial_cmp(ya, yb)),
            ),
        ];
        for (op, ours, theirs) in results {
            checked += 1;
            if ours != theirs {
                mismatches += 1;
                if mismatches <= 10 {
                    println!("{a} {op} {b}: calc {ours}, libdecNumber {theirs}");
                }
            }
        }
    }
    println!("{checked} results compared, {mismatches} mismatches");
}

#[cfg(not(all(feature = "calc", feature = "dec")))]
pub fn run(_: usize) {
    eprintln!("diff needs the calc and dec features");
}
