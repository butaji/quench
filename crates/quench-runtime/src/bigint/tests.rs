//! `JsBigInt` against `num-bigint`, its former representation, over random operands whose
//! digits favor the boundary values (all ones, top bit alone, zero) that drive the
//! division's estimate corrections and the carry chains.

use super::{Bitwise, Error, JsBigInt};
use num_bigint::BigInt;

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn digit(&mut self) -> u64 {
        match self.next() % 8 {
            0 => u64::MAX,
            1 => 1 << 63,
            2 => 0,
            3 => 1,
            _ => self.next(),
        }
    }

    fn integer(&mut self, max_digits: u64) -> BigInt {
        let len = self.next() % (max_digits + 1);
        let digits: Vec<u32> = (0..len).flat_map(|_| {
            let digit = self.digit();
            [digit as u32, (digit >> 32) as u32]
        }).collect();
        let magnitude = num_bigint::BigUint::from_slice(&digits);
        let sign = if self.next().is_multiple_of(2) { num_bigint::Sign::Plus } else { num_bigint::Sign::Minus };
        BigInt::from_biguint(sign, magnitude)
    }
}

fn js(value: &BigInt) -> JsBigInt {
    JsBigInt::from(value)
}

fn check(expected: BigInt, actual: JsBigInt, what: &str, a: &BigInt, b: &BigInt) {
    assert_eq!(actual.to_num(), expected, "{what} of {a} and {b}");
    assert_eq!(actual, js(&expected), "{what} of {a} and {b} is normalized");
}

#[test]
fn bigint_operations_match_num_bigint() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for round in 0..20_000 {
        let max_digits = if round % 10 == 0 { 40 } else { 9 };
        let (a, b) = (random.integer(max_digits), random.integer(max_digits));
        let (x, y) = (js(&a), js(&b));
        check(&a + &b, x.add(&y), "sum", &a, &b);
        check(&a - &b, x.sub(&y), "difference", &a, &b);
        check(&a * &b, x.mul(&y).unwrap(), "product", &a, &b);
        if b == BigInt::ZERO {
            assert_eq!(x.div_rem(&y, true).err(), Some(Error::DivisionByZero));
            assert_eq!(x.div_rem(&y, false).err(), Some(Error::DivisionByZero));
        } else {
            check(&a / &b, x.div_rem(&y, true).unwrap(), "quotient", &a, &b);
            check(&a % &b, x.div_rem(&y, false).unwrap(), "remainder", &a, &b);
        }
        check(&a & &b, x.bitwise(&y, Bitwise::And), "and", &a, &b);
        check(&a | &b, x.bitwise(&y, Bitwise::Or), "or", &a, &b);
        check(&a ^ &b, x.bitwise(&y, Bitwise::Xor), "xor", &a, &b);
        check(-&a, x.negate(), "negation", &a, &b);
        check(!&a, x.not(), "complement", &a, &b);
        assert_eq!(x.cmp(&y), a.cmp(&b), "order of {a} and {b}");
        assert_eq!(x == y, a == b);
        let count = BigInt::from(random.next() % 300) - 150;
        let c = js(&count);
        let shifted_left = if count < BigInt::ZERO { &a >> (-&count).to_u64_digits().1[0] } else { &a << count.to_u64_digits().1.first().copied().unwrap_or(0) };
        check(shifted_left.clone(), x.shift(&c, true).unwrap(), "left shift", &a, &count);
        check(-&count, js(&count).negate(), "count negation", &a, &count);
        let shifted_right = x.shift(&c.negate(), false).unwrap();
        check(shifted_left, shifted_right, "right shift by the negated count", &a, &count);
        assert_eq!(x.to_f64(), super::to_f64(&a), "{a} as a Number");
    }
}

#[test]
fn bigint_powers_and_limits_match_num_bigint() {
    let mut random = Random(7);
    for _ in 0..500 {
        let a = random.integer(3);
        let exponent = random.next() % 40;
        check(a.pow(exponent as u32), js(&a).pow(&JsBigInt::from(exponent as u8)).unwrap(), "power", &a, &BigInt::from(exponent));
    }
    let huge = js(&(BigInt::from(1) << 70));
    assert_eq!(js(&BigInt::from(2)).pow(&huge).err(), Some(Error::TooLarge));
    assert_eq!(js(&BigInt::from(-1)).pow(&huge).unwrap(), JsBigInt::from(1_u8));
    assert_eq!(js(&BigInt::from(-1)).pow(&js(&((BigInt::from(1) << 70) + 1))).unwrap(), js(&BigInt::from(-1)));
    assert_eq!(js(&BigInt::from(2)).pow(&JsBigInt::from(0)).unwrap(), JsBigInt::from(1_u8));
    assert_eq!(js(&BigInt::from(-2)).pow(&js(&BigInt::from(-1))).err(), Some(Error::NegativeExponent));
    let max_shift = js(&BigInt::from(super::value::MAX_BITS));
    assert_eq!(JsBigInt::from(1).shift(&max_shift, true).err(), Some(Error::TooLarge));
    assert_eq!(js(&BigInt::from(-5)).shift(&huge, false).unwrap(), js(&BigInt::from(-1)));
    assert_eq!(js(&BigInt::from(5)).shift(&huge, false).unwrap(), JsBigInt::ZERO);
    assert_eq!(JsBigInt::ZERO.shift(&huge, true).unwrap(), JsBigInt::ZERO);
}

#[test]
fn bigint_reduction_modulo_the_ed25519_prime() {
    let p = (BigInt::from(1) << 255) - 19;
    let mut random = Random(25519);
    for _ in 0..5_000 {
        let (a, b) = (random.integer(4) % &p, random.integer(4) % &p);
        let product = js(&a).mul(&js(&b)).unwrap();
        check((&a * &b) % &p, product.div_rem(&js(&p), false).unwrap(), "field product", &a, &b);
    }
}
