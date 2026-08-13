use crate::{ops::Builtin, value::Value};

pub(crate) fn property(key: &str) -> Option<Builtin> {
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
    let numbers: Vec<f64> = arguments.iter().map(number).collect();
    if let Some(value) = transcendental(builtin, &numbers) {
        return Ok(Value::Number(value));
    }
    let value = match builtin {
        Builtin::MathAbs => numbers.first().copied().unwrap_or(f64::NAN).abs(),
        Builtin::MathFloor => numbers.first().copied().unwrap_or(f64::NAN).floor(),
        Builtin::MathCeil => numbers.first().copied().unwrap_or(f64::NAN).ceil(),
        Builtin::MathRound => round(numbers.first().copied().unwrap_or(f64::NAN)),
        Builtin::MathTrunc => numbers.first().copied().unwrap_or(f64::NAN).trunc(),
        Builtin::MathMax => numbers.into_iter().fold(f64::NEG_INFINITY, f64::max),
        Builtin::MathMin => numbers.into_iter().fold(f64::INFINITY, f64::min),
        Builtin::MathSign => numbers.first().copied().unwrap_or(f64::NAN).signum(),
        Builtin::MathSqrt => numbers.first().copied().unwrap_or(f64::NAN).sqrt(),
        Builtin::MathCbrt => numbers.first().copied().unwrap_or(f64::NAN).cbrt(),
        Builtin::MathHypot => numbers
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt(),
        Builtin::MathImul => imul(numbers.first(), numbers.get(1)),
        Builtin::MathClz32 => f64::from(clz32(numbers.first().copied().unwrap_or(f64::NAN))),
        Builtin::MathFround => f64::from(numbers.first().copied().unwrap_or(f64::NAN) as f32),
        Builtin::MathF16Round => {
            crate::value::f16_round(numbers.first().copied().unwrap_or(f64::NAN))
        }
        Builtin::MathRandom => 0.5,
        _ => return Err(crate::execute::VmError::NotCallable),
    };
    Ok(Value::Number(value))
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
    let left = left.copied().unwrap_or(f64::NAN) as i32;
    let right = right.copied().unwrap_or(f64::NAN) as i32;
    left.wrapping_mul(right) as f64
}

fn clz32(value: f64) -> u32 {
    crate::construct::to_uint32(value).leading_zeros()
}

fn sum_precise(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let iterable = value.cloned().ok_or_else(|| {
        crate::value::error::throw_type_error("Math.sumPrecise requires an iterable")
    })?;
    let mut sum = -0.0f64;
    let mut compensation = 0.0f64;
    crate::collections::iterator::for_each_iterable(iterable, |value| {
        let Value::Number(value) = value else {
            return Err(crate::value::error::throw_type_error(
                "Math.sumPrecise requires numbers",
            ));
        };
        if value.is_nan() || sum.is_nan() {
            sum = f64::NAN;
            return Ok(());
        }
        if !value.is_finite() {
            if sum.is_infinite() && sum.is_sign_positive() != value.is_sign_positive() {
                sum = f64::NAN;
            } else if !sum.is_infinite() {
                sum = value;
            }
            compensation = 0.0;
            return Ok(());
        }
        if sum.is_infinite() {
            return Ok(());
        }
        let total = sum + value;
        compensation += if sum.abs() >= value.abs() {
            (sum - total) + value
        } else {
            (value - total) + sum
        };
        sum = total;
        Ok(())
    })?;
    let result = if compensation == 0.0 {
        sum
    } else {
        sum + compensation
    };
    Ok(Value::Number(result))
}

fn unary(numbers: &[f64], operation: fn(f64) -> f64) -> f64 {
    operation(numbers.first().copied().unwrap_or(f64::NAN))
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Number(value) => *value,
        Value::Null => 0.0,
        Value::Boolean(value) => f64::from(*value),
        Value::String(value) => value.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn round(value: f64) -> f64 {
    if value.fract() == 0.5 {
        (value - 0.5).ceil()
    } else {
        value.round()
    }
}
