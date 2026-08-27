pub(crate) fn array_copy_within(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let object = crate::construct::to_object(receiver.unwrap_or(&Value::Undefined))?;
    let original_length = crate::arrays::array_like_length(&object)?;
    let target = copy_index(arguments.first(), original_length)?;
    let start = copy_index(arguments.get(1), original_length)?;
    let explicit_end = arguments
        .get(2)
        .filter(|value| !matches!(value, Value::Undefined))
        .map(|value| copy_index(Some(value), original_length))
        .transpose()?;
    let current = crate::locals::resolved_replacement(object.clone());
    let Value::Array(values) = &current else {
        let end = explicit_end.unwrap_or(original_length);
        let count = end
            .saturating_sub(start)
            .min(original_length.saturating_sub(target));
        return copy_within_object(current, target, start, count);
    };
    let receiver = &current;
    let end = explicit_end.unwrap_or(original_length);
    let count = end
        .saturating_sub(start)
        .min(original_length.saturating_sub(target));

    // A custom prototype or indexed accessor makes HasProperty observable;
    // keep those receivers on the ordinary property path.
    if values.prototype().is_some() || values.has_indexed_accessor() {
        return copy_within_object(current, target, start, count);
    }

    // The dense backing store is canonical only for packed ordinary arrays.
    // In that state copy_dense_within supplies memmove ordering without a
    // temporary Vec; all other representations retain the property-aware
    // clone path below.
    if values.is_packed_ordinary() {
        let mut updated = values.clone();
        debug_assert!(Rc::make_mut(&mut updated).copy_dense_within(start, target, count));
        let result = Value::Array(updated);
        crate::locals::replace_value(receiver, &result);
        return Ok(result);
    }

    let mut updated = values.as_ref().clone();
    if target < start {
        for offset in 0..count {
            copy_dense_property(&mut updated, start + offset, target + offset);
        }
    } else {
        for offset in (0..count).rev() {
            copy_dense_property(&mut updated, start + offset, target + offset);
        }
    }
    let result = Value::Array(std::rc::Rc::new(updated));
    crate::locals::replace_value(receiver, &result);
    Ok(result)
}

pub(crate) fn typed_array_copy_within(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = crate::arrays::typed_array_receiver(receiver, "copyWithin")?;
    if crate::arrays::typed_array_is_immutable(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.copyWithin called on immutable buffer",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let target = copy_index(arguments.first(), length)?;
    let start = copy_index(arguments.get(1), length)?;
    let end = arguments
        .get(2)
        .filter(|value| !matches!(value, Value::Undefined))
        .map(|value| copy_index(Some(value), length))
        .transpose()?
        .unwrap_or(length);
    let count = end.saturating_sub(start).min(length.saturating_sub(target));
    if count == 0 {
        return Ok(value);
    }
    if crate::arrays::typed_array_is_detached(&value)
        || crate::typed_array_prototype::is_out_of_bounds(&value)
    {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.copyWithin called on invalid view",
        ));
    }

    // Snapshot before writing so overlapping ranges preserve the source bits.
    let source = (0..count)
        .map(|offset| {
            let index = start + offset;
            crate::typed_array_prototype::index_exists(&value, index)
                .then(|| crate::execute::get_property_result(&value, &index.to_string()))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (offset, item) in source.into_iter().enumerate() {
        let destination = target + offset;
        // A length-tracking view can shrink during argument coercion. Writes
        // beyond its current extent are ignored, while fixed-length OOB views
        // were rejected above.
        if let Some(item) = item {
            if !matches!(item, Value::Undefined)
                && crate::typed_array_prototype::index_exists(&value, destination)
            {
                crate::properties::assign_set_property(&value, &destination.to_string(), item)?;
            }
        }
    }
    Ok(value)
}

fn copy_within_object(
    mut target: Value,
    destination: usize,
    source: usize,
    count: usize,
) -> Result<Value, crate::execute::VmError> {
    let forward = destination < source;
    let offsets: Box<dyn Iterator<Item = usize>> = if forward {
        Box::new(0..count)
    } else {
        Box::new((0..count).rev())
    };
    for offset in offsets {
        let from = (source + offset).to_string();
        let to = (destination + offset).to_string();
        if crate::with_scope::has_property(&target, &from)? {
            let value = crate::execute::get_property_result(&target, &from)?;
            let updated = crate::properties::assign_set_property(&target, &to, value)?;
            crate::locals::replace_value(&target, &updated);
            target = updated;
        } else {
            let (updated, deleted) = if matches!(target, Value::Proxy(_)) {
                let result = crate::proxy::proxy_delete(&target, &to)?;
                (target.clone(), crate::execute::is_truthy(&result))
            } else {
                crate::execute::delete_property(target.clone(), &to)
            };
            if !deleted {
                return Err(crate::value::error::throw_type_error(
                    "Cannot delete property during copyWithin",
                ));
            }
            crate::locals::replace_value(&target, &updated);
            target = updated;
        }
    }
    Ok(target)
}

fn copy_dense_property(values: &mut crate::value::ArrayData, source: usize, destination: usize) {
    if source < values.logical_len() && values.has_index(source) {
        if let Some(value) = values.get_index(source) {
            values.set_index(destination, value);
        }
    } else {
        values.delete_property(&destination.to_string());
    }
}

fn copy_index(value: Option<&Value>, length: usize) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(0);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() {
        return Ok(0);
    }
    if number.is_sign_negative() {
        return Ok(length.saturating_sub(number.abs().floor() as usize));
    }
    Ok((number.floor() as usize).min(length))
}
