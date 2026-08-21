//! `toLocaleString` family and number/string formatting helpers.
use super::{resolve_locales, runtime_error, to_string_value};
use crate::{execute::VmError, ops::Builtin, value::Value};

mod date_kind;
pub(crate) mod parse_num;
pub(crate) use date_kind::DateLocaleKind;
mod array_values;
mod date;
mod locale_number;
include!("tolocale_value.rs");
include!("tolocale_symbol.rs");

/// `Array.prototype.toLocaleString`.
pub(crate) fn array_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let values = match receiver {
        Some(Value::Array(values)) => values.iter().cloned().collect(),
        Some(value) => array_values::typed_values(value).ok_or_else(|| {
            crate::value::error::throw_type_error(
                "Array.prototype.toLocaleString called on non-array",
            )
        })?,
        None => {
            return Err(crate::value::error::throw_type_error(
                "called on null or undefined",
            ))
        }
    };
    let mut parts = Vec::new();
    for value in values.iter() {
        parts.push(element_to_locale_string(value, arguments)?);
    }
    Ok(Value::String(parts.join(",")))
}

include!("tolocale_bigint.rs");

pub(crate) fn dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::StringLocaleCompare => string_locale_compare(receiver, arguments),
        Builtin::ArrayToLocaleString => array_to_locale_string(receiver, arguments),
        Builtin::NumberToLocaleString => locale_number::to_locale_string(receiver, arguments),
        Builtin::StringToLocaleLowerCase => string_to_locale_case(receiver, arguments, false),
        Builtin::StringToLocaleUpperCase => string_to_locale_case(receiver, arguments, true),
        Builtin::DateToLocaleString => {
            date::to_locale_string(DateLocaleKind::String, receiver, arguments)
        }
        Builtin::DateToLocaleDateString => {
            date::to_locale_string(DateLocaleKind::Date, receiver, arguments)
        }
        Builtin::DateToLocaleTimeString => {
            date::to_locale_string(DateLocaleKind::Time, receiver, arguments)
        }
        _ => return None,
    };
    Some(result)
}

fn string_locale_compare(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(crate::vm::not_callable)?;
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "String.prototype.localeCompare called on null or undefined",
        ));
    }
    let left = crate::conversion::to_string(receiver)?;
    let right =
        crate::conversion::to_string(arguments.first().map_or(&Value::Undefined, |value| value))?;
    let collator_arguments = arguments
        .get(1..)
        .map_or_else(Vec::new, |values| values.to_vec());
    crate::intl::collator::construct(&collator_arguments)?;
    let result = match left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => match (left == right, left.cmp(&right)) {
            (true, _) => 0.0,
            (false, std::cmp::Ordering::Less) => 1.0,
            (false, std::cmp::Ordering::Greater) => -1.0,
            (false, std::cmp::Ordering::Equal) => 0.0,
        },
        std::cmp::Ordering::Greater => 1.0,
    };
    Ok(Value::Number(result))
}

fn element_to_locale_string(value: &Value, arguments: &[Value]) -> Result<String, VmError> {
    match value {
        Value::Number(_) => locale_element_call(value, arguments),
        Value::Null | Value::Undefined => Ok(String::new()),
        _ => locale_element_call(value, arguments),
    }
}
fn locale_element_call(value: &Value, arguments: &[Value]) -> Result<String, VmError> {
    let method = crate::execute::get_property_result(value, "toLocaleString")?;
    if !matches!(method, Value::Undefined | Value::Null) {
        let invoke_arguments = vec![
            arguments.first().cloned().unwrap_or(Value::Undefined),
            arguments.get(1).cloned().unwrap_or(Value::Undefined),
        ];
        let result =
            crate::functions::execute_target_with_receiver(&method, value, &invoke_arguments)?;
        return crate::conversion::to_string(&result.0);
    }
    Ok(to_string_value(value))
}
pub(crate) fn string_to_locale_case(
    receiver: Option<&Value>,
    arguments: &[Value],
    upper: bool,
) -> Result<Value, VmError> {
    let value = receiver.ok_or_else(crate::vm::not_callable)?;
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(runtime_error("TypeError: String.prototype.toLocale*Case"));
    }
    let value = crate::conversion::to_string(value)?;
    let locale = resolve_locales(arguments)?
        .first()
        .cloned()
        .unwrap_or_default();
    let result = case::locale_case(&value, &locale, upper);
    Ok(Value::String(result))
}
mod case;
