/// The UTF-16 units of the search-string argument, rejecting regular
/// expressions as includes/startsWith/endsWith require.
fn search_string(arguments: &[Value]) -> Result<Vec<u16>, crate::execute::VmError> {
    let value = argument_value(arguments);
    if crate::regexp::is_regexp(value)? {
        return Err(crate::value::error::throw_type_error(
            "First argument must not be a regular expression",
        ));
    }
    Ok(crate::conversion::to_string(value)?.encode_utf16().collect())
}

pub(crate) fn includes(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let search = search_string(arguments)?;
    let start = search_position(arguments.get(1), units.len())?;
    Ok(Value::Boolean(contains_from(&units, &search, start)))
}

/// The UTF-16 code units of the receiver, unwrapping boxed strings.
fn receiver_units(receiver: Option<&Value>) -> Result<Vec<u16>, crate::execute::VmError> {
    if let Some(Value::StringUnits(units)) = receiver {
        return Ok((**units).clone());
    }
    Ok(string_receiver(receiver)?.encode_utf16().collect())
}

/// ToIntegerOrInfinity of an optional position argument, clamped to `length`.
fn search_position(value: Option<&Value>, length: usize) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(0);
    };
    let position = crate::conversion::to_number(value)?;
    if position.is_nan() || position <= 0.0 {
        return Ok(0);
    }
    Ok(position.trunc().min(length as f64) as usize)
}

fn contains_from(units: &[u16], search: &[u16], start: usize) -> bool {
    search.is_empty()
        || units
            .get(start..)
            .is_some_and(|tail| tail.windows(search.len()).any(|window| window == search))
}

pub(crate) fn starts_with(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let search = search_string(arguments)?;
    let start = search_position(arguments.get(1), units.len())?;
    Ok(Value::Boolean(
        units
            .get(start..)
            .is_some_and(|tail| tail.starts_with(&search)),
    ))
}

pub(crate) fn ends_with(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let search = search_string(arguments)?;
    let end = match arguments.get(1) {
        None => units.len(),
        value => search_position(value, units.len())?,
    };
    Ok(Value::Boolean(units[..end].ends_with(&search)))
}

/// Whether `c` is ECMAScript WhiteSpace or LineTerminator: Rust's
/// `is_whitespace` plus U+FEFF (zero width no-break space).
fn is_js_whitespace(c: char) -> bool {
    c == '\u{FEFF}' || c.is_whitespace()
}

pub(crate) fn trim(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(
        string_receiver(receiver)?
            .trim_matches(is_js_whitespace)
            .to_string(),
    ))
}

pub(crate) fn index_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let search: Vec<u16> = crate::conversion::to_string(argument_value(arguments))?
        .encode_utf16()
        .collect();
    let start = search_position(arguments.get(1), units.len())?;
    let index = find_units(&units[start..], &search).map(|index| start + index);
    Ok(Value::Number(index.map_or(-1.0, |index| index as f64)))
}

pub(crate) fn last_index_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let search: Vec<u16> = crate::conversion::to_string(argument_value(arguments))?
        .encode_utf16()
        .collect();
    let max_start = units.len().saturating_sub(search.len());
    let start = match arguments.get(1) {
        None => max_start,
        Some(value) => {
            let position = crate::conversion::to_number(value)?;
            if position.is_nan() {
                max_start
            } else if position <= 0.0 {
                0
            } else {
                (position.trunc() as usize).min(max_start)
            }
        }
    };
    let index = rfind_units(&units, &search, start);
    Ok(Value::Number(index.map_or(-1.0, |index| index as f64)))
}

fn argument_value(arguments: &[Value]) -> &Value {
    arguments.first().unwrap_or(&Value::Undefined)
}

/// The last index at or before `max_start` where `pattern` occurs.
fn rfind_units(units: &[u16], pattern: &[u16], max_start: usize) -> Option<usize> {
    if pattern.is_empty() {
        return Some(max_start.min(units.len()));
    }
    if pattern.len() > units.len() {
        return None;
    }
    (0..=max_start.min(units.len() - pattern.len()))
        .rev()
        .find(|index| units[*index..].starts_with(pattern))
}

fn find_units(units: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    units
        .windows(pattern.len())
        .position(|window| window == pattern)
}

pub(crate) fn trim_start(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(
        string_receiver(receiver)?
            .trim_start_matches(is_js_whitespace)
            .to_string(),
    ))
}

pub(crate) fn trim_end(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(
        string_receiver(receiver)?
            .trim_end_matches(is_js_whitespace)
            .to_string(),
    ))
}

pub(crate) fn split(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let limit = split_limit(arguments)?;
    let pattern = match arguments.first() {
        None | Some(Value::Undefined) => None,
        Some(value) => Some(crate::conversion::to_string(value)?),
    };
    if limit == 0 {
        return Ok(Value::array(Vec::new()));
    }
    let Some(pattern) = pattern else {
        return Ok(Value::array(vec![from_units(units)]));
    };
    Ok(split_pattern(&units, &pattern, limit))
}

/// The split limit: `ToUint32(limit)` or `2^32 - 1` when undefined.
fn split_limit(arguments: &[Value]) -> Result<u32, crate::execute::VmError> {
    match arguments.get(1) {
        None | Some(Value::Undefined) => Ok(u32::MAX),
        Some(value) => Ok(crate::construct::to_uint32(crate::conversion::to_number(
            value,
        )?)),
    }
}

fn split_pattern(units: &[u16], pattern: &str, limit: u32) -> Value {
    let pattern: Vec<u16> = pattern.encode_utf16().collect();
    if pattern.is_empty() {
        let values = units
            .iter()
            .take(limit as usize)
            .map(|unit| from_units(vec![*unit]))
            .collect();
        return Value::array(values);
    }
    let mut values = Vec::new();
    let mut rest = units;
    while values.len() + 1 < limit as usize {
        let Some(index) = find_units(rest, &pattern) else {
            break;
        };
        values.push(from_units(rest[..index].to_vec()));
        rest = &rest[index + pattern.len()..];
    }
    values.push(from_units(rest.to_vec()));
    Value::array(values)
}
