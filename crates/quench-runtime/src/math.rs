use num_bigint::{BigInt, Sign};

use crate::{ops::Builtin, value::Value};

pub(crate) fn property(key: &str) -> Option<Builtin> {
    property_core(key).or_else(|| match key {
        "clz32" => Some(Builtin::MathClz32),
        "cosh" => Some(Builtin::MathCosh),
        "expm1" => Some(Builtin::MathExpm1),
        "fround" => Some(Builtin::MathFround),
        "log1p" => Some(Builtin::MathLog1p),
        "sinh" => Some(Builtin::MathSinh),
        "tanh" => Some(Builtin::MathTanh),
        "f16round" => Some(Builtin::MathF16Round),
        "random" => Some(Builtin::MathRandom),
        "sumPrecise" => Some(Builtin::MathSumPrecise),
        _ => None,
    })
}

fn property_core(key: &str) -> Option<Builtin> {
    match key {
        "pow" => Some(Builtin::MathPow),
        "abs" => Some(Builtin::MathAbs),
        "floor" => Some(Builtin::MathFloor),
        "ceil" => Some(Builtin::MathCeil),
        "round" => Some(Builtin::MathRound),
        "trunc" => Some(Builtin::MathTrunc),
        "max" => Some(Builtin::MathMax),
        "min" => Some(Builtin::MathMin),
        "sign" => Some(Builtin::MathSign),
        "sqrt" => Some(Builtin::MathSqrt),
        "cbrt" => Some(Builtin::MathCbrt),
        "hypot" => Some(Builtin::MathHypot),
        "imul" => Some(Builtin::MathImul),
        "log" => Some(Builtin::MathLog),
        "log10" => Some(Builtin::MathLog10),
        "log2" => Some(Builtin::MathLog2),
        "exp" => Some(Builtin::MathExp),
        "sin" => Some(Builtin::MathSin),
        "cos" => Some(Builtin::MathCos),
        "tan" => Some(Builtin::MathTan),
        "asin" => Some(Builtin::MathAsin),
        "acos" => Some(Builtin::MathAcos),
        "atan" => Some(Builtin::MathAtan),
        "atan2" => Some(Builtin::MathAtan2),
        "acosh" => Some(Builtin::MathAcosh),
        "asinh" => Some(Builtin::MathAsinh),
        "atanh" => Some(Builtin::MathAtanh),
        _ => None,
    }
}

pub(crate) fn constant(key: &str) -> Option<Value> {
    let value = match key {
        "E" => std::f64::consts::E,
        "LN2" => std::f64::consts::LN_2,
        "LN10" => std::f64::consts::LN_10,
        "LOG2E" => std::f64::consts::LOG2_E,
        "LOG10E" => std::f64::consts::LOG10_E,
        "PI" => std::f64::consts::PI,
        "SQRT1_2" => std::f64::consts::FRAC_1_SQRT_2,
        "SQRT2" => std::f64::consts::SQRT_2,
        _ => return None,
    };
    Some(Value::Number(value))
}

pub(crate) fn is_builtin(builtin: Builtin) -> bool {
    property_name(builtin).is_some()
}

fn property_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::MathAbs => Some("abs"),
        Builtin::MathFloor => Some("floor"),
        Builtin::MathCeil => Some("ceil"),
        Builtin::MathRound => Some("round"),
        Builtin::MathTrunc => Some("trunc"),
        Builtin::MathMax => Some("max"),
        Builtin::MathMin => Some("min"),
        Builtin::MathSign => Some("sign"),
        Builtin::MathSqrt => Some("sqrt"),
        Builtin::MathCbrt => Some("cbrt"),
        Builtin::MathHypot => Some("hypot"),
        Builtin::MathImul => Some("imul"),
        Builtin::MathLog => Some("log"),
        Builtin::MathLog10 => Some("log10"),
        Builtin::MathLog2 => Some("log2"),
        Builtin::MathExp => Some("exp"),
        Builtin::MathSin => Some("sin"),
        Builtin::MathCos => Some("cos"),
        Builtin::MathTan => Some("tan"),
        Builtin::MathAsin => Some("asin"),
        Builtin::MathAcos => Some("acos"),
        Builtin::MathAtan => Some("atan"),
        Builtin::MathAtan2 => Some("atan2"),
        Builtin::MathAcosh => Some("acosh"),
        Builtin::MathAsinh => Some("asinh"),
        Builtin::MathAtanh => Some("atanh"),
        Builtin::MathClz32 => Some("clz32"),
        Builtin::MathCosh => Some("cosh"),
        Builtin::MathExpm1 => Some("expm1"),
        Builtin::MathFround => Some("fround"),
        Builtin::MathLog1p => Some("log1p"),
        Builtin::MathSinh => Some("sinh"),
        Builtin::MathTanh => Some("tanh"),
        Builtin::MathF16Round => Some("f16round"),
        Builtin::MathRandom => Some("random"),
        Builtin::MathSumPrecise => Some("sumPrecise"),
        _ => None,
    }
}

pub(crate) fn execute(
    builtin: Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if builtin == Builtin::MathSumPrecise {
        return sum_precise(arguments.first());
    }
    let numbers: Vec<f64> = arguments
        .iter()
        .map(crate::conversion::to_number)
        .collect::<Result<_, _>>()?;
    if let Some(value) = transcendental(builtin, &numbers) {
        return Ok(Value::Number(value));
    }
    let value = match discrete_math(builtin, &numbers) {
        Some(value) => value,
        None => return Err(crate::execute::VmError::NotCallable),
    };
    Ok(Value::Number(value))
}

fn discrete_math(builtin: Builtin, numbers: &[f64]) -> Option<f64> {
    Some(match builtin {
        Builtin::MathAbs => numbers.first().copied().unwrap_or(f64::NAN).abs(),
        Builtin::MathFloor => numbers.first().copied().unwrap_or(f64::NAN).floor(),
        Builtin::MathCeil => numbers.first().copied().unwrap_or(f64::NAN).ceil(),
        Builtin::MathRound => round(numbers.first().copied().unwrap_or(f64::NAN)),
        Builtin::MathTrunc => numbers.first().copied().unwrap_or(f64::NAN).trunc(),
        Builtin::MathMax => extrema(&numbers, f64::NEG_INFINITY, f64::max),
        Builtin::MathMin => extrema(&numbers, f64::INFINITY, f64::min),
        Builtin::MathSign => sign(numbers.first().copied().unwrap_or(f64::NAN)),
        Builtin::MathSqrt => numbers.first().copied().unwrap_or(f64::NAN).sqrt(),
        Builtin::MathCbrt => numbers.first().copied().unwrap_or(f64::NAN).cbrt(),
        Builtin::MathHypot => hypot(&numbers),
        Builtin::MathImul => imul(numbers.first(), numbers.get(1)),
        Builtin::MathClz32 => f64::from(clz32(numbers.first().copied().unwrap_or(f64::NAN))),
        Builtin::MathFround => f64::from(numbers.first().copied().unwrap_or(f64::NAN) as f32),
        Builtin::MathF16Round => {
            crate::value::f16_round(numbers.first().copied().unwrap_or(f64::NAN))
        }
        Builtin::MathRandom => 0.5,
        _ => return None,
    })
}

fn transcendental(builtin: Builtin, numbers: &[f64]) -> Option<f64> {
    let value = match builtin {
        Builtin::MathLog => unary(numbers, f64::ln),
        Builtin::MathLog10 => unary(numbers, f64::log10),
        Builtin::MathLog2 => unary(numbers, f64::log2),
        Builtin::MathExp => unary(numbers, f64::exp),
        Builtin::MathSin => unary(numbers, f64::sin),
        Builtin::MathCos => unary(numbers, f64::cos),
        Builtin::MathTan => unary(numbers, f64::tan),
        Builtin::MathAsin => unary(numbers, f64::asin),
        Builtin::MathAcos => unary(numbers, f64::acos),
        Builtin::MathAtan => unary(numbers, f64::atan),
        Builtin::MathAcosh => unary(numbers, f64::acosh),
        Builtin::MathAsinh => unary(numbers, f64::asinh),
        Builtin::MathAtanh => unary(numbers, f64::atanh),
        Builtin::MathCosh => unary(numbers, f64::cosh),
        Builtin::MathExpm1 => unary(numbers, f64::exp_m1),
        Builtin::MathLog1p => unary(numbers, f64::ln_1p),
        Builtin::MathSinh => unary(numbers, f64::sinh),
        Builtin::MathTanh => unary(numbers, f64::tanh),
        Builtin::MathAtan2 => numbers
            .first()
            .copied()
            .unwrap_or(f64::NAN)
            .atan2(numbers.get(1).copied().unwrap_or(f64::NAN)),
        _ => return None,
    };
    Some(value)
}

fn imul(left: Option<&f64>, right: Option<&f64>) -> f64 {
    let left = crate::construct::to_int32(left.copied().unwrap_or(f64::NAN));
    let right = crate::construct::to_int32(right.copied().unwrap_or(f64::NAN));
    left.wrapping_mul(right) as f64
}

fn extrema(numbers: &[f64], initial: f64, compare: fn(f64, f64) -> f64) -> f64 {
    numbers
        .iter()
        .copied()
        .try_fold(initial, |result, value| {
            (!value.is_nan()).then_some(compare(result, value))
        })
        .unwrap_or(f64::NAN)
}

fn hypot(numbers: &[f64]) -> f64 {
    if numbers.iter().any(|value| value.is_infinite()) {
        return f64::INFINITY;
    }
    if numbers.iter().any(|value| value.is_nan()) {
        return f64::NAN;
    }
    let largest = numbers.iter().copied().map(f64::abs).fold(0.0, f64::max);
    if largest == 0.0 {
        return 0.0;
    }
    let squares = numbers
        .iter()
        .map(|value| (value / largest).powi(2))
        .sum::<f64>();
    largest * squares.sqrt()
}

fn sign(value: f64) -> f64 {
    if value == 0.0 {
        value
    } else {
        value.signum()
    }
}

fn clz32(value: f64) -> u32 {
    crate::construct::to_uint32(value).leading_zeros()
}

fn sum_precise(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let iterable = value.cloned().ok_or_else(|| {
        crate::value::error::throw_type_error("Math.sumPrecise requires an iterable")
    })?;
    let mut sum = BigInt::from(0_u8);
    let mut state = SumState::default();
    crate::collections::iterator::for_each_iterable(iterable, |value| {
        let Value::Number(value) = value else {
            return Err(crate::value::error::throw_type_error(
                "Math.sumPrecise requires numbers",
            ));
        };
        state.add(value, &mut sum);
        Ok(())
    })?;
    Ok(Value::Number(state.finish(sum)))
}

#[derive(Default)]
struct SumState {
    infinity: Option<bool>,
    invalid: bool,
    positive_zero: bool,
    nonzero: bool,
}

impl SumState {
    fn add(&mut self, value: f64, sum: &mut BigInt) {
        if value.is_nan() {
            self.invalid = true;
        } else if value.is_infinite() {
            self.add_infinity(value.is_sign_positive());
        } else if value == 0.0 {
            self.positive_zero |= value.is_sign_positive();
        } else {
            self.nonzero = true;
            *sum += binary_units(value);
        }
    }

    fn add_infinity(&mut self, positive: bool) {
        self.invalid |= self.infinity.is_some_and(|sign| sign != positive);
        self.infinity.get_or_insert(positive);
    }

    fn finish(self, sum: BigInt) -> f64 {
        if self.invalid {
            f64::NAN
        } else if let Some(positive) = self.infinity {
            if positive {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            }
        } else if sum == BigInt::from(0_u8) {
            if !self.nonzero && !self.positive_zero {
                -0.0
            } else {
                0.0
            }
        } else {
            scaled_binary_units(sum)
        }
    }
}

fn binary_units(value: f64) -> BigInt {
    let bits = value.to_bits();
    let fraction = bits & ((1_u64 << 52) - 1);
    let exponent = ((bits >> 52) & 0x7ff) as usize;
    let significand = if exponent == 0 {
        fraction
    } else {
        fraction | (1_u64 << 52)
    };
    let units = BigInt::from(significand) << exponent.saturating_sub(1);
    if value.is_sign_negative() {
        -units
    } else {
        units
    }
}

fn scaled_binary_units(sum: BigInt) -> f64 {
    let negative = sum.sign() == Sign::Minus;
    let magnitude = if negative { -sum } else { sum };
    let bits = magnitude.magnitude().bits() as usize;
    let result = if bits <= 52 {
        bigint_to_f64(&magnitude) * 2_f64.powi(-1074)
    } else {
        let shift = bits - 53;
        let significand = rounded_significand(&magnitude, shift);
        bigint_to_f64(&significand) * 2_f64.powi(shift as i32 - 1074)
    };
    if negative {
        -result
    } else {
        result
    }
}

fn rounded_significand(magnitude: &BigInt, shift: usize) -> BigInt {
    let mut significand = magnitude >> shift;
    if shift == 0 {
        return significand;
    }
    let remainder = magnitude - (&significand << shift);
    let halfway = BigInt::from(1_u8) << (shift - 1);
    if remainder > halfway
        || remainder == halfway && (&significand & BigInt::from(1_u8)) != BigInt::from(0_u8)
    {
        significand += 1_u8;
    }
    significand
}

fn bigint_to_f64(value: &BigInt) -> f64 {
    value.to_string().parse().unwrap_or(f64::NAN)
}

fn unary(numbers: &[f64], operation: fn(f64) -> f64) -> f64 {
    operation(numbers.first().copied().unwrap_or(f64::NAN))
}

fn round(value: f64) -> f64 {
    if value.is_nan() || value.is_infinite() || value == 0.0 {
        return value;
    }
    if value.fract() == 0.0 {
        return value;
    }
    let lower = value.floor();
    let result = if value - lower < 0.5 {
        lower
    } else {
        lower + 1.0
    };
    if result == 0.0 && value < 0.0 {
        -0.0
    } else {
        result
    }
}
