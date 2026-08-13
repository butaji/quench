pub(crate) fn slice(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    if matches!(this, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.slice called on null or undefined",
        ));
    }
    let length = slice_length(&this)?;
    let start = slice_index(arguments.first(), length, 0)?;
    let end = slice_index(arguments.get(1), length, length)?;
    let count = (end - start).max(0);
    let species = slice_species(&this, count)?;
    if species.is_none() && count > 4_294_967_295 {
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    copy_slice(&this, start, end, species)
}

/// `LengthOfArrayLike`: `ToLength` of the receiver's `length` property.
fn slice_length(this: &Value) -> Result<isize, crate::execute::VmError> {
    let length = crate::execute::get_property_result(this, "length")?;
    let number = crate::conversion::to_number(&length)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.trunc().min(9_007_199_254_740_991.0) as isize)
}

/// `ToIntegerOrInfinity` of an optional index argument, clamped relative to
/// `length`; `None`/undefined yields `default`.
fn slice_index(
    value: Option<&Value>,
    length: isize,
    default: isize,
) -> Result<isize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(default);
    };
    if matches!(value, Value::Undefined) {
        return Ok(default);
    }
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() {
        return Ok(0.min(length));
    }
    if number.is_infinite() {
        return Ok(if number < 0.0 { 0 } else { length });
    }
    let integer = number.trunc() as isize;
    Ok(if integer < 0 {
        (length + integer).max(0)
    } else {
        integer.min(length)
    })
}

/// `ArraySpeciesCreate`: the species-constructed result, or `None` for the
/// default plain array of `count` holes.
fn slice_species(this: &Value, count: isize) -> Result<Option<Value>, crate::execute::VmError> {
    if !matches!(this, Value::Array(_)) {
        return Ok(None);
    }
    let constructor = crate::locals::resolved_replacement(
        crate::execute::get_property_result(this, "constructor")?,
    );
    if matches!(constructor, Value::Undefined) {
        return Ok(None);
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "Species constructor is not a constructor",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    crate::construct::construct_value(&species, &[Value::Number(count as f64)]).map(Some)
}

fn copy_slice(
    this: &Value,
    start: isize,
    end: isize,
    species: Option<Value>,
) -> Result<Value, crate::execute::VmError> {
    let mut elements = Vec::new();
    let mut holes = Vec::new();
    for key in start..end {
        let key = key.to_string();
        if crate::with_scope::has_property(this, &key)? {
            elements.push(crate::execute::get_property_result(this, &key)?);
        } else {
            holes.push(elements.len());
            elements.push(Value::Undefined);
        }
    }
    apply_slice(elements, holes, species)
}

fn apply_slice(
    elements: Vec<Value>,
    holes: Vec<usize>,
    species: Option<Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(target) = species else {
        let mut data = crate::value::ArrayData::new(elements);
        for index in holes {
            data.delete_property(&index.to_string());
        }
        return Ok(Value::Array(std::rc::Rc::new(data)));
    };
    let mut target = target;
    let length = elements.len();
    for (index, value) in elements.into_iter().enumerate() {
        if !holes.contains(&index) {
            target = crate::builtins::define_own_property(
                &target,
                &index.to_string(),
                &[
                    ("value".to_string(), value),
                    ("writable".to_string(), Value::Boolean(true)),
                    ("enumerable".to_string(), Value::Boolean(true)),
                    ("configurable".to_string(), Value::Boolean(true)),
                ],
            )?;
        }
    }
    Ok(crate::builtins::set_property(
        target,
        "length",
        Value::Number(length as f64),
    ))
}
