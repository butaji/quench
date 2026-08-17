use crate::{execute::VmError, value::Value};

pub(super) fn total_time_out_of_range(values: &[f64]) -> bool {
    let scales = [
        86_400_000_000_000_i128,
        3_600_000_000_000,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = values
        .iter()
        .zip(scales)
        .try_fold(0_i128, |total, (value, scale)| {
            let value = i128::from(*value as i64);
            total.checked_add(value.checked_mul(scale)?)
        });
    let limit = 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
    total.is_none_or(|total| total.abs() > limit)
}

pub(super) fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0.0)
}

pub(super) fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = match value {
        Some(Value::Number(value)) => Ok(*value),
        Some(Value::Undefined) | None => Ok(0.0),
        Some(value) => crate::conversion::to_number(value),
    }?;
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Duration fields must be integral",
        ));
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}
