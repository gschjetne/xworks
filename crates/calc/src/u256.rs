//! Unsigned 256-bit integers: just enough for decimal arithmetic's exact
//! intermediates, such as the product of two 34-digit coefficients or a sum
//! aligned across exponents, up to 77 digits.

use std::cmp::Ordering;

/// An unsigned 256-bit integer, as four 64-bit limbs, least significant
/// first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct U256([u64; 4]);

/// 10^19, the largest power of ten in a `u64`.
const TEN_19: u64 = 10_000_000_000_000_000_000;

impl U256 {
    pub(crate) fn from_u128(x: u128) -> U256 {
        U256([x as u64, (x >> 64) as u64, 0, 0])
    }

    /// The value as a `u128`, if it fits.
    pub(crate) fn to_u128(self) -> Option<u128> {
        let [a, b, c, d] = self.0;
        (c == 0 && d == 0).then_some(a as u128 | (b as u128) << 64)
    }

    pub(crate) fn is_zero(self) -> bool {
        self.0 == [0; 4]
    }

    /// The full product of two `u128`s.
    pub(crate) fn mul_u128(a: u128, b: u128) -> U256 {
        let a = [a as u64, (a >> 64) as u64];
        let b = [b as u64, (b >> 64) as u64];
        let mut r = [0u64; 4];
        for i in 0..2 {
            let mut carry = 0u128;
            for j in 0..2 {
                let t = a[i] as u128 * b[j] as u128 + r[i + j] as u128 + carry;
                r[i + j] = t as u64;
                carry = t >> 64;
            }
            r[i + 2] = carry as u64;
        }
        U256(r)
    }

    /// `self × m`, or `None` if it overflows.
    fn mul_u64(self, m: u64) -> Option<U256> {
        let mut r = [0u64; 4];
        let mut carry = 0u128;
        for (i, limb) in self.0.iter().enumerate() {
            let t = *limb as u128 * m as u128 + carry;
            r[i] = t as u64;
            carry = t >> 64;
        }
        (carry == 0).then_some(U256(r))
    }

    /// `self × 10^n`, or `None` if it overflows.
    pub(crate) fn mul_pow10(self, mut n: u32) -> Option<U256> {
        let mut v = self;
        while n >= 19 {
            v = v.mul_u64(TEN_19)?;
            n -= 19;
        }
        if n > 0 {
            v = v.mul_u64(10u64.pow(n))?;
        }
        Some(v)
    }

    /// `self + o`. The sum must fit.
    pub(crate) fn add(self, o: U256) -> U256 {
        let mut r = [0u64; 4];
        let mut carry = false;
        for (i, slot) in r.iter_mut().enumerate() {
            let (s, c1) = self.0[i].overflowing_add(o.0[i]);
            let (s, c2) = s.overflowing_add(carry as u64);
            *slot = s;
            carry = c1 || c2;
        }
        debug_assert!(!carry, "U256 addition overflowed");
        U256(r)
    }

    /// `self − o`. `self` must be at least `o`.
    pub(crate) fn sub(self, o: U256) -> U256 {
        let mut r = [0u64; 4];
        let mut borrow = false;
        for (i, slot) in r.iter_mut().enumerate() {
            let (s, b1) = self.0[i].overflowing_sub(o.0[i]);
            let (s, b2) = s.overflowing_sub(borrow as u64);
            *slot = s;
            borrow = b1 || b2;
        }
        debug_assert!(!borrow, "U256 subtraction underflowed");
        U256(r)
    }

    /// The quotient and remainder of `self ÷ d`. `d` must not be zero.
    pub(crate) fn div_rem_u64(self, d: u64) -> (U256, u64) {
        let mut q = [0u64; 4];
        let mut rem = 0u128;
        for i in (0..4).rev() {
            let cur = rem << 64 | self.0[i] as u128;
            q[i] = (cur / d as u128) as u64;
            rem = cur % d as u128;
        }
        (U256(q), rem as u64)
    }

    /// `self ÷ 10^n`, rounded down, and whether anything was left over.
    pub(crate) fn div_pow10(self, mut n: u32) -> (U256, bool) {
        let mut v = self;
        let mut rest = false;
        while n >= 19 {
            let (q, r) = v.div_rem_u64(TEN_19);
            v = q;
            rest |= r != 0;
            n -= 19;
        }
        if n > 0 {
            let (q, r) = v.div_rem_u64(10u64.pow(n));
            v = q;
            rest |= r != 0;
        }
        (v, rest)
    }

    /// How many decimal digits the value has; zero has one.
    pub(crate) fn digits(self) -> u32 {
        let mut v = self;
        let mut count = 0;
        loop {
            if let Some(x) = v.to_u128() {
                return count + digits_u128(x);
            }
            v = v.div_rem_u64(TEN_19).0;
            count += 19;
        }
    }
}

impl Ord for U256 {
    fn cmp(&self, o: &U256) -> Ordering {
        self.0.iter().rev().cmp(o.0.iter().rev())
    }
}

impl PartialOrd for U256 {
    fn partial_cmp(&self, o: &U256) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// How many decimal digits `x` has; zero has one.
pub(crate) fn digits_u128(x: u128) -> u32 {
    if x == 0 { 1 } else { x.ilog10() + 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_are_exact() {
        let max = u128::MAX;
        // (2^128 − 1)² = 2^256 − 2^129 + 1
        let p = U256::mul_u128(max, max);
        assert_eq!(p.0, [1, 0, u64::MAX - 1, u64::MAX]);
        let ten34 = 10u128.pow(34);
        assert_eq!(U256::mul_u128(ten34, ten34).digits(), 69);
    }

    #[test]
    fn powers_of_ten_go_up_and_down() {
        let v = U256::from_u128(123).mul_pow10(70).unwrap();
        assert_eq!(v.digits(), 73);
        assert_eq!(v.div_pow10(70), (U256::from_u128(123), false));
        assert!(U256::from_u128(1).mul_pow10(78).is_none());
        let (q, rest) = U256::from_u128(12_345).div_pow10(2);
        assert_eq!((q.to_u128(), rest), (Some(123), true));
    }

    #[test]
    fn sums_differences_and_order() {
        let a = U256::from_u128(u128::MAX);
        let b = U256::from_u128(1);
        let s = a.add(b);
        assert_eq!(s.0, [0, 0, 1, 0]);
        assert_eq!(s.sub(b), a);
        assert!(s > a && b < a);
    }

    #[test]
    fn digits_count() {
        assert_eq!(digits_u128(0), 1);
        assert_eq!(digits_u128(9), 1);
        assert_eq!(digits_u128(10), 2);
        assert_eq!(digits_u128(u128::MAX), 39);
    }
}
