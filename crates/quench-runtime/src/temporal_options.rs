//! Shared Temporal option facts.
//!
//! Option names and their accepted values are semantic data, so every
//! Temporal family member uses the same validator instead of carrying a
//! subtly different copy.

use crate::{execute::VmError, value::Value};

pub(crate) const OVERFLOW_VALUES: [&str; 2] = ["constrain", "reject"];
pub(crate) const DISAMBIGUATION_VALUES: [&str; 4] = ["compatible", "earlier", "later", "reject"];

pub(crate) fn overflow(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(OVERFLOW_VALUES[0].to_string());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "overflow")?;
    if matches!(value, Value::Undefined) {
        return Ok(OVERFLOW_VALUES[0].to_string());
    }
    let value = crate::conversion::to_string(&value)?;
    if OVERFLOW_VALUES.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(crate::value::error::throw_range_error("Invalid overflow"))
    }
}

pub(crate) fn reject_overflow(options: Option<&Value>) -> Result<bool, VmError> {
    Ok(overflow(options)? == OVERFLOW_VALUES[1])
}

pub(crate) fn constrain_overflow(options: Option<&Value>) -> Result<bool, VmError> {
    Ok(overflow(options)? == OVERFLOW_VALUES[0])
}

pub(crate) fn disambiguation(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(DISAMBIGUATION_VALUES[0].to_string());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "disambiguation")?;
    if matches!(value, Value::Undefined) {
        return Ok(DISAMBIGUATION_VALUES[0].to_string());
    }
    if crate::conversion::is_symbol(&value) {
        return Err(crate::value::error::throw_type_error(
            "Invalid disambiguation",
        ));
    }
    let value = crate::conversion::to_string(&value)?;
    if DISAMBIGUATION_VALUES.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(crate::value::error::throw_range_error(
            "Invalid disambiguation",
        ))
    }
}
