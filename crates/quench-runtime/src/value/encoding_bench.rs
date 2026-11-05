//! Compare encoding algorithms and call boundaries in one executable. These are
//! measurement alternatives, not production selection rules. Linear TOD-9 owns results.
extern crate test;

use super::*;
use std::hint::black_box;
use test::Bencher;

use crate::number_inputs::Inputs;

trait NumberEncoding {
    fn number(number: f64) -> Value;
}

struct Roundtrip;
struct Ieee;
struct Truncated;

impl NumberEncoding for Roundtrip {
    #[inline(always)]
    fn number(number: f64) -> Value { Value::number(number) }
}

#[inline(always)]
fn canonical_double(number: f64) -> Value {
    Value(if number.is_nan() { CANONICAL_NAN } else { number.to_bits() })
}

impl NumberEncoding for Ieee {
    #[inline(always)]
    fn number(number: f64) -> Value {
        let bits = number.to_bits();
        let exponent = ((bits >> F64_FRACTION_BITS) & F64_EXPONENT_MASK)
            .wrapping_sub(F64_EXPONENT_BIAS as u64);
        const INT32_MAGNITUDE_EXPONENTS: u64 = (i32::BITS - 1) as u64;
        if exponent < INT32_MAGNITUDE_EXPONENTS && bits & (F64_FRACTION_MASK >> exponent) == 0 {
            return Value::integer(truncate_to_i32(number));
        }
        if bits == 0.0_f64.to_bits() {
            return Value::integer(0);
        }
        if bits == f64::from(i32::MIN).to_bits() {
            return Value::integer(i32::MIN);
        }
        canonical_double(number)
    }
}

impl NumberEncoding for Truncated {
    #[inline(always)]
    fn number(number: f64) -> Value {
        if number >= f64::from(i32::MIN)
            && number <= f64::from(i32::MAX)
            && number.trunc() == number
            && number.to_bits() != (-0.0_f64).to_bits()
        {
            Value::integer(truncate_to_i32(number))
        } else {
            canonical_double(number)
        }
    }
}

#[inline(never)]
fn outlined<E: NumberEncoding>(number: f64) -> Value {
    E::number(number)
}

#[inline(always)]
fn encode<E: NumberEncoding, const OUTLINED: bool>(number: f64) -> Value {
    if OUTLINED {
        outlined::<E>(number)
    } else {
        E::number(number)
    }
}

fn throughput<E: NumberEncoding, const OUTLINED: bool>(b: &mut Bencher, inputs: Inputs) {
    let numbers = inputs.numbers();
    for number in numbers {
        assert_eq!(encode::<E, OUTLINED>(number), Value::number(number));
    }
    b.iter(|| {
        for number in numbers {
            black_box(encode::<E, OUTLINED>(black_box(number)));
        }
    });
}

fn chain<E: NumberEncoding, const OUTLINED: bool>(b: &mut Bencher, fractional: bool) {
    // Integral sums remain in i32 range. The fractional affine recurrence remains
    // bounded below magnitude one. Both carry the encoded value across each step.
    let numbers = if fractional { Inputs::UnitFractions } else { Inputs::Integers }.numbers();
    let factor = if fractional { 0.5 } else { 1.0 };
    let run = || {
        let mut state = Value::integer(0);
        for number in numbers {
            let next = black_box(state).as_number().unwrap() * black_box(factor) + black_box(number);
            state = black_box(encode::<E, OUTLINED>(next));
        }
        state
    };
    let mut expected = Value::integer(0);
    for number in numbers {
        expected = Value::number(expected.as_number().unwrap() * factor + number);
    }
    assert_eq!(run(), expected);
    b.iter(|| black_box(run()));
    assert_eq!(run(), expected);
}

macro_rules! encoders {
    ($kernel:ident, $input:expr; $round:ident, $round_call:ident, $ieee:ident, $ieee_call:ident, $trunc:ident, $trunc_call:ident) => {
        #[bench] fn $round(b: &mut Bencher) { $kernel::<Roundtrip, false>(b, $input); }
        #[bench] fn $round_call(b: &mut Bencher) { $kernel::<Roundtrip, true>(b, $input); }
        #[bench] fn $ieee(b: &mut Bencher) { $kernel::<Ieee, false>(b, $input); }
        #[bench] fn $ieee_call(b: &mut Bencher) { $kernel::<Ieee, true>(b, $input); }
        #[bench] fn $trunc(b: &mut Bencher) { $kernel::<Truncated, false>(b, $input); }
        #[bench] fn $trunc_call(b: &mut Bencher) { $kernel::<Truncated, true>(b, $input); }
    };
}

encoders!(throughput, Inputs::Integers; primitive_encoding_int_round, primitive_encoding_int_round_call, primitive_encoding_int_ieee, primitive_encoding_int_ieee_call, primitive_encoding_int_trunc, primitive_encoding_int_trunc_call);
encoders!(throughput, Inputs::WideIntegers; primitive_encoding_wide_round, primitive_encoding_wide_round_call, primitive_encoding_wide_ieee, primitive_encoding_wide_ieee_call, primitive_encoding_wide_trunc, primitive_encoding_wide_trunc_call);
encoders!(throughput, Inputs::Fractions; primitive_encoding_fraction_round, primitive_encoding_fraction_round_call, primitive_encoding_fraction_ieee, primitive_encoding_fraction_ieee_call, primitive_encoding_fraction_trunc, primitive_encoding_fraction_trunc_call);
encoders!(throughput, Inputs::UnitFractions; primitive_encoding_unit_round, primitive_encoding_unit_round_call, primitive_encoding_unit_ieee, primitive_encoding_unit_ieee_call, primitive_encoding_unit_trunc, primitive_encoding_unit_trunc_call);
encoders!(throughput, Inputs::Dyadic; primitive_encoding_dyadic_round, primitive_encoding_dyadic_round_call, primitive_encoding_dyadic_ieee, primitive_encoding_dyadic_ieee_call, primitive_encoding_dyadic_trunc, primitive_encoding_dyadic_trunc_call);
encoders!(throughput, Inputs::Mixed; primitive_encoding_mixed_round, primitive_encoding_mixed_round_call, primitive_encoding_mixed_ieee, primitive_encoding_mixed_ieee_call, primitive_encoding_mixed_trunc, primitive_encoding_mixed_trunc_call);
encoders!(throughput, Inputs::Boundaries; primitive_encoding_edge_round, primitive_encoding_edge_round_call, primitive_encoding_edge_ieee, primitive_encoding_edge_ieee_call, primitive_encoding_edge_trunc, primitive_encoding_edge_trunc_call);
encoders!(chain, false; primitive_encoding_chain_int_round, primitive_encoding_chain_int_round_call, primitive_encoding_chain_int_ieee, primitive_encoding_chain_int_ieee_call, primitive_encoding_chain_int_trunc, primitive_encoding_chain_int_trunc_call);
encoders!(chain, true; primitive_encoding_chain_fraction_round, primitive_encoding_chain_fraction_round_call, primitive_encoding_chain_fraction_ieee, primitive_encoding_chain_fraction_ieee_call, primitive_encoding_chain_fraction_trunc, primitive_encoding_chain_fraction_trunc_call);

#[test]
fn primitive_encoding_algorithms_preserve_all_exponents_and_mantissa_boundaries() {
    fn check(number: f64) {
        let expected = Value::number(number);
        assert_eq!(Ieee::number(number), expected, "{:016x}", number.to_bits());
        assert_eq!(Truncated::number(number), expected, "{:016x}", number.to_bits());
        assert_eq!(outlined::<Ieee>(number), expected);
        assert_eq!(outlined::<Truncated>(number), expected);
        assert_eq!(outlined::<Roundtrip>(number), expected);
    }
    for exponent in 0..=F64_EXPONENT_MASK {
        for sign in [0, 1_u64 << (u64::BITS - 1)] {
            for bit in 0..F64_FRACTION_BITS {
                let power = 1_u64 << bit;
                for fraction in [0, power - 1, power, power + 1] {
                    let number = f64::from_bits(sign | (exponent << F64_FRACTION_BITS) | fraction);
                    check(number);
                }
            }
        }
    }
    for number in Inputs::Boundaries.numbers() {
        check(number);
        check(number.next_up());
        check(number.next_down());
    }
    const SAMPLE_COUNT: usize = 100_000;
    // An odd Weyl increment spans the word space before repeating. Bound this
    // diagnostic sample, then separately visit the exact integer represented by it.
    const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut bits = 0_u64;
    for _ in 0..SAMPLE_COUNT {
        bits = bits.wrapping_add(SAMPLE_STEP);
        check(f64::from_bits(bits));
        check(f64::from(bits as i32));
    }
}
