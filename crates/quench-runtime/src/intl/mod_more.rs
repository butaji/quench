fn supported_currencies() -> Vec<Value> {
    strings(CURRENCIES)
}

fn supported_numbering_systems() -> Vec<Value> {
    strings(&["latn"])
}

fn supported_time_zones() -> Vec<Value> {
    strings(&["UTC"])
}

const UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

fn supported_units() -> Vec<Value> {
    strings(UNITS)
}

fn strings(values: &[&str]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::String(value.to_string()))
        .collect()
}

fn runtime_error(message: &str) -> VmError {
    if let Some(message) = message.strip_prefix("TypeError: ") {
        return crate::value::error::throw_type_error(message);
    }
    if let Some(message) = message.strip_prefix("RangeError: ") {
        return crate::value::error::throw_range_error(message);
    }
    VmError::EvalError(message.to_string())
}

/// Return the internal slot map of an Intl object as an owned vector.
pub(crate) fn intl_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    let Some(Value::Object(properties)) = receiver else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    let Some((_, Value::Object(slots))) = properties.iter().find(|(name, _)| name == SLOT) else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    Ok(slots.properties.clone())
}

pub(crate) fn slot_string(slots: &[(String, Value)], key: &str) -> Option<String> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
}

pub(crate) fn slot_bool(slots: &[(String, Value)], key: &str) -> Option<bool> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Boolean(value) => Some(*value),
            _ => None,
        })
}

pub(crate) fn slot_number(slots: &[(String, Value)], key: &str) -> Option<f64> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
}

pub(crate) fn make_object(properties: Vec<(String, Value)>) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
}

pub(crate) fn make_array(values: Vec<Value>) -> Value {
    Value::array(values)
}
