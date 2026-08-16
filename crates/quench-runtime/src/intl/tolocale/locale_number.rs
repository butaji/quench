use crate::{execute::VmError, value::Value};

use super::runtime_error;

pub(super) fn to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let value = match crate::vm::number_value_of(receiver)? {
        Value::Number(number) => Value::Number(number),
        _ => return Err(runtime_error("TypeError: Number.prototype.toLocaleString")),
    };
    let formatter = crate::intl::number::construct(arguments)?;
    crate::intl::number::prototype_method(
        crate::ops::Builtin::IntlNumberFormatFormat,
        &[value],
        Some(&formatter),
    )
}
