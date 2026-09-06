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
    if species.is_none() && (count as u64) > 4_294_967_295u64 {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    copy_slice(&this, start, end, species)
}

fn typed_array_slice(this: &Value, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    if !this.is_typed_array() {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on incompatible receiver",
        ));
    }
    let value = this.clone();
    let detached = crate::arrays::typed_array_is_detached(&value);
    if detached {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on detached buffer",
        ));
    }
    if !detached && crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on out-of-bounds view",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0) as isize;
    let start = slice_index(arguments.first(), length, 0)?;
    let end = slice_index(arguments.get(1), length, length)?;
    let count = (end - start).max(0) as usize;
    let target = crate::arrays::typed_array_species_create(&value, count)?;

    // The source length is captured before species construction. A fixed view
    // that becomes out of bounds must throw; a length-tracking view simply
    // contributes undefined values for elements no longer available.
    if (crate::arrays::typed_array_is_detached(&value)
        || crate::typed_array_prototype::is_out_of_bounds(&value))
        && count > 0
    {
        return Err(crate::value::error::throw_type_error(
            "TypedArray source became out of bounds",
        ));
    }
    let current_length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    for index in 0..count {
        let source_index = start as usize + index;
        if source_index < current_length {
            let item = crate::execute::get_property_result(&value, &source_index.to_string())?;
            copy_slice_element(&target, index, item)?;
        }
    }
    Ok(target)
}

fn copy_slice_element(
    target: &Value,
    index: usize,
    item: Value,
) -> Result<(), crate::execute::VmError> {
    if let Value::Uint8Array(view) = target {
        if view.buffer.immutable {
            let number = crate::conversion::to_number(&item)?;
            if !view.set_intrinsic(index, crate::construct::to_uint8(number)) {
                return Err(crate::value::error::throw_type_error(
                    "TypedArray slice target is out of bounds",
                ));
            }
            return Ok(());
        }
    }
    crate::properties::assign_set_property(target, &index.to_string(), item).map(|_| ())
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
    if !matches!(crate::builtins::is_array(Some(this))?, Value::Boolean(true)) {
        return Ok(None);
    }
    let constructor = crate::locals::resolved_replacement(crate::execute::get_property_result(
        this,
        "constructor",
    )?);
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
    let Some(target) = species else {
        return copy_default_slice(this, start, end);
    };
    copy_species_slice(this, start, end, target)
}

fn copy_default_slice(
    this: &Value,
    start: isize,
    end: isize,
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
    let mut data = crate::value::ArrayData::new(elements);
    for index in holes {
        data.delete_property(&index.to_string());
    }
    Ok(Value::Array(std::rc::Rc::new(data)))
}

fn copy_species_slice(
    this: &Value,
    start: isize,
    end: isize,
    target: Value,
) -> Result<Value, crate::execute::VmError> {
    let mut target = crate::locals::resolved_replacement(target);
    let mut destination = 0usize;
    for source in start..end {
        let key = source.to_string();
        if crate::with_scope::has_property(this, &key)? {
            let value = crate::execute::get_property_result(this, &key)?;
            target = crate::builtins::create_data_property_or_throw(
                target,
                &destination.to_string(),
                value,
            )?;
        }
        destination = destination.saturating_add(1);
    }
    crate::properties::assign_set_property(
        &target,
        "length",
        Value::Number(destination as f64),
    )
}
