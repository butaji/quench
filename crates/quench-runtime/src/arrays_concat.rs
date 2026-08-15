pub(crate) fn concat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    if matches!(this, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.concat called on null or undefined",
        ));
    }
    let species = concat_species(&this)?;
    let mut items = vec![this];
    items.extend(arguments.iter().cloned());
    if species.is_none() && sparse_array_items(&items)? {
        return Ok(concat_sparse_arrays(&items)?);
    }
    let mut elements = Vec::new();
    let mut holes = Vec::new();
    for item in &items {
        spread_concat_element(&mut elements, &mut holes, item)?;
    }
    if species.is_none() {
        let mut data = crate::value::ArrayData::new(elements);
        for index in holes {
            data.delete_property(&index.to_string());
        }
        return Ok(Value::Array(std::rc::Rc::new(data)));
    }
    let hole = |index: usize| holes.contains(&index);
    let mut target = species.unwrap_or_else(|| Value::array(Vec::new()));
    let length = elements.len();
    for (index, value) in elements.into_iter().enumerate() {
        if !hole(index) {
            target = crate::builtins::set_property(target, &index.to_string(), value);
        }
    }
    Ok(crate::builtins::set_property(
        target,
        "length",
        Value::Number(length as f64),
    ))
}

fn sparse_array_items(items: &[Value]) -> Result<bool, crate::execute::VmError> {
    items.iter().try_fold(true, |all_arrays, item| {
        Ok(all_arrays && matches!(item, Value::Array(_)) && is_concat_spreadable(item)?)
    })
}

fn concat_sparse_arrays(items: &[Value]) -> Result<Value, crate::execute::VmError> {
    let length = items.iter().try_fold(0usize, |total, item| {
        let Value::Array(values) = item else {
            return Ok(total);
        };
        total
            .checked_add(values.logical_len())
            .ok_or_else(|| crate::value::error::throw_type_error("Maximum array size exceeded"))
    })?;
    let mut result = crate::value::ArrayData::new(Vec::new());
    result.set_length(length);
    let mut offset = 0;
    for item in items {
        let Value::Array(values) = item else { continue };
        let mut index = 0;
        while let Some(current) = values.next_index(index, values.logical_len()) {
            if let Some(value) = values.get_index(current) {
                result.set_index(offset + current, value);
            }
            index = current.saturating_add(1);
        }
        offset += values.logical_len();
    }
    Ok(Value::Array(std::rc::Rc::new(result)))
}

/// `ArraySpeciesCreate`: the species-constructed result, or `None` for the
/// default plain array.
fn concat_species(this: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    if !matches!(this, Value::Array(_)) {
        return Ok(None);
    }
    let constructor = crate::execute::get_property_result(this, "constructor")?;
    if matches!(constructor, Value::Undefined) {
        return Ok(None);
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "Species constructor is not a constructor",
        ));
    }
    if matches!(constructor, Value::Builtin(crate::ops::Builtin::Array)) {
        return Ok(None);
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    crate::construct::construct_value(&species, &[Value::Number(0.0)]).map(Some)
}

fn spread_concat_element(
    elements: &mut Vec<Value>,
    holes: &mut Vec<usize>,
    item: &Value,
) -> Result<(), crate::execute::VmError> {
    if !is_concat_spreadable(item)? {
        elements.push(item.clone());
        return Ok(());
    }
    let length = concat_array_like_length(item)?;
    if elements.len() + length > 9_007_199_254_740_991 {
        return Err(crate::value::error::throw_type_error(
            "Maximum array size exceeded",
        ));
    }
    for index in 0..length {
        let key = index.to_string();
        let value = if crate::with_scope::has_property(item, &key)? {
            crate::execute::get_property_result(item, &key)?
        } else {
            holes.push(elements.len());
            Value::Undefined
        };
        elements.push(value);
    }
    Ok(())
}

fn is_concat_spreadable(value: &Value) -> Result<bool, crate::execute::VmError> {
    if !crate::value::is_object(value) {
        return Ok(false);
    }
    let flag = crate::execute::get_property_result(value, "Symbol.isConcatSpreadable")?;
    if !matches!(flag, Value::Undefined) {
        return Ok(crate::intl::tolocale::value::is_truthy(&flag));
    }
    Ok(matches!(value, Value::Array(_)) || proxy_targets_array(value))
}

fn proxy_targets_array(value: &Value) -> bool {
    let Value::Proxy(proxy) = value else {
        return false;
    };
    matches!(&proxy.target, Value::Array(_))
}

fn concat_array_like_length(value: &Value) -> Result<usize, crate::execute::VmError> {
    let length = crate::execute::get_property_result(value, "length")?;
    let number = crate::conversion::to_number(&length)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}
