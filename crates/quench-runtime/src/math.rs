use crate::{ops::Builtin, value::Value};

pub(crate) fn execute(
    builtin: Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let numbers: Vec<f64> = arguments.iter().map(number).collect();
    let value = match builtin {
        Builtin::MathAbs => numbers.first().copied().unwrap_or(f64::NAN).abs(),
        Builtin::MathFloor => numbers.first().copied().unwrap_or(f64::NAN).floor(),
        Builtin::MathCeil => numbers.first().copied().unwrap_or(f64::NAN).ceil(),
        Builtin::MathRound => round(numbers.first().copied().unwrap_or(f64::NAN)),
        Builtin::MathTrunc => numbers.first().copied().unwrap_or(f64::NAN).trunc(),
        Builtin::MathMax => numbers.into_iter().fold(f64::NEG_INFINITY, f64::max),
        Builtin::MathMin => numbers.into_iter().fold(f64::INFINITY, f64::min),
        _ => return Err(crate::execute::VmError::NotCallable),
    };
    Ok(Value::Number(value))
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
