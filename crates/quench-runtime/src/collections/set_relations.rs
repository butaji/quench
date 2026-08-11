enum SetRecord {
    Native {
        values: Vec<Value>,
        source: Option<Value>,
    },
    Like {
        receiver: Value,
        size: f64,
        has: Value,
        keys: Value,
    },
}

impl SetRecord {
    fn size(&self) -> f64 {
        match self {
            Self::Native { values, .. } => values.len() as f64,
            Self::Like { size, .. } => *size,
        }
    }

    fn contains(&self, value: &Value) -> Result<bool, VmError> {
        match self {
            Self::Native { values, .. } => {
                Ok(values.iter().any(|item| same_value_zero(item, value)))
            }
            Self::Like { receiver, has, .. } => {
                let result = crate::functions::execute_target(
                    has,
                    receiver,
                    std::slice::from_ref(value),
                )?;
                Ok(crate::execute::is_truthy(&result))
            }
        }
    }

    fn keys(&self) -> Result<Vec<Value>, VmError> {
        match self {
            Self::Native { values, source } => {
                if let Some(Value::Set(data)) = source {
                    return Ok(data.values.borrow().iter().cloned().collect());
                }
                Ok(values.clone())
            }
            Self::Like { receiver, keys, .. } => {
                let iterator = crate::functions::execute_target(keys, receiver, &[])?;
                crate::collections::iterator::collect_iterator_object(iterator)
            }
        }
    }
}

pub(crate) fn set_relation(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let own = native_receiver(receiver)?;
    let other = set_record(arguments.first().cloned().unwrap_or(Value::Undefined))?;
    match builtin {
        Builtin::SetDifference => difference(&own, &other),
        Builtin::SetIntersection => intersection(&own, &other),
        Builtin::SetSymmetricDifference => symmetric_difference(&own, &other),
        Builtin::SetUnion => union(&own, &other),
        Builtin::SetIsDisjointFrom => disjoint(&own, &other),
        Builtin::SetIsSubsetOf => subset(&own, &other),
        Builtin::SetIsSupersetOf => superset(&own, &other),
        _ => Err(VmError::MissingReturn),
    }
}

fn native_receiver(receiver: Option<&Value>) -> Result<SetRecord, VmError> {
    let Some(Value::Set(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    if data.weak {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    }
    Ok(SetRecord::Native {
        values: data.values.borrow().iter().cloned().collect(),
        source: Some(Value::Set(Rc::clone(data))),
    })
}

fn set_record(value: Value) -> Result<SetRecord, VmError> {
    if let Value::Set(data) = &value {
        let values = data.values.borrow().iter().cloned().collect();
        return Ok(SetRecord::Native {
            values,
            source: Some(value),
        });
    }
    let size = crate::conversion::to_number(&crate::execute::get_property_result(&value, "size")?)?;
    if !size.is_finite() || size < 0.0 {
        return Err(crate::value::error::throw_type_error(
            "Set-like size must be a non-negative number",
        ));
    }
    let has = crate::execute::get_property_result(&value, "has")?;
    let keys = crate::execute::get_property_result(&value, "keys")?;
    if !crate::conversion::is_callable(&has) {
        return Err(crate::value::error::throw_type_error(
            "Set-like has is not callable",
        ));
    }
    if !crate::conversion::is_callable(&keys) {
        return Err(crate::value::error::throw_type_error(
            "Set-like keys is not callable",
        ));
    }
    Ok(SetRecord::Like {
        receiver: value,
        size,
        has,
        keys,
    })
}

fn difference(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    let values = own_values(own);
    if own.size() <= other.size() {
        return filter_values(values, |value| other.contains(value).map(|contains| !contains));
    }
    let mut result = values;
    for value in other.keys()? {
        result.retain(|item| !same_value_zero(item, &value));
    }
    Ok(new_set(result))
}

fn intersection(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    if own.size() <= other.size() {
        return filter_values(own_values(own), |value| other.contains(value));
    }
    let own_values = own_values(own);
    let result = other
        .keys()?
        .into_iter()
        .filter(|value| own_values.iter().any(|item| same_value_zero(item, value)))
        .collect();
    Ok(new_set(result))
}

fn symmetric_difference(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    let own_values = own_values(own);
    let other_values = other.keys()?;
    let mut result: Vec<Value> = own_values
        .iter()
        .filter(|value| !other_values.iter().any(|item| same_value_zero(item, value)))
        .cloned()
        .collect();
    result.extend(
        other_values
            .into_iter()
            .filter(|value| !own_values.iter().any(|item| same_value_zero(item, value))),
    );
    Ok(new_set(result))
}

fn union(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    let mut result = own_values(own);
    for value in other.keys()? {
        if !result.iter().any(|item| same_value_zero(item, &value)) {
            result.push(value);
        }
    }
    Ok(new_set(result))
}

fn disjoint(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    if own.size() <= other.size() {
        return Ok(Value::Boolean(all_values(&own_values(own), |value| {
            other.contains(value).map(|contains| !contains)
        })?));
    }
    let own_values = own_values(own);
    Ok(Value::Boolean(!other
        .keys()?
        .iter()
        .any(|value| own_values.iter().any(|item| same_value_zero(item, value)))))
}

fn subset(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    if own.size() > other.size() {
        return Ok(Value::Boolean(false));
    }
    Ok(Value::Boolean(all_values(&own_values(own), |value| {
        other.contains(value)
    })?))
}

fn superset(own: &SetRecord, other: &SetRecord) -> Result<Value, VmError> {
    if own.size() < other.size() {
        return Ok(Value::Boolean(false));
    }
    let own_values = own_values(own);
    Ok(Value::Boolean(
        other
            .keys()?
            .iter()
            .all(|value| own_values.iter().any(|item| same_value_zero(item, value))),
    ))
}

fn own_values(record: &SetRecord) -> Vec<Value> {
    match record {
        SetRecord::Native { values, source } => {
            if let Some(Value::Set(data)) = source {
                return data.values.borrow().iter().cloned().collect();
            }
            values.clone()
        }
        SetRecord::Like { .. } => Vec::new(),
    }
}

fn all_values<F>(values: &[Value], mut predicate: F) -> Result<bool, VmError>
where
    F: FnMut(&Value) -> Result<bool, VmError>,
{
    for value in values {
        if !predicate(value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn filter_values<F>(values: Vec<Value>, mut predicate: F) -> Result<Value, VmError>
where
    F: FnMut(&Value) -> Result<bool, VmError>,
{
    let mut result = Vec::new();
    for value in values {
        if predicate(&value)? {
            result.push(value);
        }
    }
    Ok(new_set(result))
}

fn new_set(values: Vec<Value>) -> Value {
    Value::Set(Rc::new(SetData {
        weak: false,
        values: std::cell::RefCell::new(values.into_iter().collect()),
        prototype: std::cell::RefCell::new(None),
    }))
}
