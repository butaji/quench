//! The `++`/`--` step of a Number operand, which every tier's update shares.

use crate::value::Value;

/// The `++`/`--` step: one, negated for a decrement.
fn increment_delta(is_decrement: bool) -> f64 {
    if is_decrement { -1.0 } else { 1.0 }
}

/// `++`/`--` of a Number operand, staying an integer while the result fits; `None` for any
/// other operand, whose conversion may run guest code or allocate.
#[inline(always)]
pub(in crate::vm) fn number_increment(input: Value, is_decrement: bool) -> Option<Value> {
    if let Some(integer) = input.as_int() {
        let next = if is_decrement {
            integer.checked_sub(1)
        } else {
            integer.checked_add(1)
        };
        return Some(next.map_or_else(
            || Value::overflowed_integer(f64::from(integer) + increment_delta(is_decrement)),
            Value::integer,
        ));
    }
    input
        .as_number()
        .map(|number| Value::number(number + increment_delta(is_decrement)))
}

/// `number_increment` of an integer whose update stays one; `None` for any other value or
/// an overflow, which `number_increment` answers.
#[inline(always)]
pub(in crate::vm) fn integer_increment(input: Value, is_decrement: bool) -> Option<Value> {
    let integer = input.as_int()?;
    if is_decrement { integer.checked_sub(1) } else { integer.checked_add(1) }.map(Value::integer)
}
