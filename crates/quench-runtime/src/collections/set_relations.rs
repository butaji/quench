enum SetRecord {
    Native {
        data: Rc<SetData>,
    },
    Like {
        receiver: Value,
        size: f64,
        has: Value,
        keys: Value,
    },
}

enum Fold<T> {
    Next(T),
    Stop(T),
}

impl SetRecord {
    fn size(&self) -> f64 {
        match self {
            Self::Native { data } => data.values.borrow().len() as f64,
            Self::Like { size, .. } => *size,
        }
    }

    fn contains(&self, value: &Value) -> Result<bool, VmError> {
        match self {
            Self::Native { data } => Ok(data
                .values
                .borrow()
                .iter()
                .any(|item| same_value_zero(item, value))),
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

fn native_receiver(receiver: Option<&Value>) -> Result<Rc<SetData>, VmError> {
    let Some(Value::Set(data)) = receiver.filter(|value| matches!(value, Value::Set(d) if !d.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    Ok(Rc::clone(data))
}

fn set_record(value: Value) -> Result<SetRecord, VmError> {
    if let Value::Set(data) = &value {
        if !data.weak {
            return Ok(SetRecord::Native {
                data: Rc::clone(data),
            });
        }
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

fn difference(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    if own.values.borrow().len() as f64 <= other.size() {
        let mut result = Vec::new();
        for_each_live(own, &mut |value| {
            if !other.contains(value)? {
                result.push(value.clone());
            }
            Ok(true)
        })?;
        return Ok(new_set(result));
    }
    let mut result: Vec<Value> = own.values.borrow().iter().cloned().collect();
    fold_keys(other, (), &mut |(), value| {
        result.retain(|item| !same_value_zero(item, value));
        Ok(Fold::Next(()))
    })?;
    Ok(new_set(result))
}

fn intersection(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    let mut result: Vec<Value> = Vec::new();
    if own.values.borrow().len() as f64 <= other.size() {
        for_each_live(own, &mut |value| {
            if other.contains(value)? && !result.iter().any(|item| same_value_zero(item, value)) {
                result.push(value.clone());
            }
            Ok(true)
        })?;
        return Ok(new_set(result));
    }
    fold_keys(other, (), &mut |(), value| {
        if native_contains(own, value) && !result.iter().any(|item| same_value_zero(item, value)) {
            result.push(value.clone());
        }
        Ok(Fold::Next(()))
    })?;
    Ok(new_set(result))
}

fn symmetric_difference(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    let mut result: Vec<Value> = own.values.borrow().iter().cloned().collect();
    fold_keys(other, (), &mut |(), value| {
        if native_contains(own, value) {
            result.retain(|item| !same_value_zero(item, value));
        } else if !result.iter().any(|item| same_value_zero(item, value)) {
            result.push(value.clone());
        }
        Ok(Fold::Next(()))
    })?;
    Ok(new_set(result))
}

fn union(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    let mut result: Vec<Value> = own.values.borrow().iter().cloned().collect();
    fold_keys(other, (), &mut |(), value| {
        if !result.iter().any(|item| same_value_zero(item, value)) {
            result.push(value.clone());
        }
        Ok(Fold::Next(()))
    })?;
    Ok(new_set(result))
}

fn disjoint(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    if own.values.borrow().len() as f64 <= other.size() {
        let mut disjoint = true;
        for_each_live(own, &mut |value| {
            if other.contains(value)? {
                disjoint = false;
                return Ok(false);
            }
            Ok(true)
        })?;
        return Ok(Value::Boolean(disjoint));
    }
    let found = fold_keys(other, false, &mut |_, value| {
        Ok(if native_contains(own, value) {
            Fold::Stop(true)
        } else {
            Fold::Next(false)
        })
    })?;
    Ok(Value::Boolean(!found))
}

fn subset(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    if (own.values.borrow().len() as f64) > other.size() {
        return Ok(Value::Boolean(false));
    }
    let mut is_subset = true;
    for_each_live(own, &mut |value| {
        if !other.contains(value)? {
            is_subset = false;
            return Ok(false);
        }
        Ok(true)
    })?;
    Ok(Value::Boolean(is_subset))
}

fn superset(own: &Rc<SetData>, other: &SetRecord) -> Result<Value, VmError> {
    if (own.values.borrow().len() as f64) < other.size() {
        return Ok(Value::Boolean(false));
    }
    let missing = fold_keys(other, false, &mut |_, value| {
        Ok(if native_contains(own, value) {
            Fold::Next(false)
        } else {
            Fold::Stop(true)
        })
    })?;
    Ok(Value::Boolean(!missing))
}

fn native_contains(data: &Rc<SetData>, value: &Value) -> bool {
    data.values
        .borrow()
        .iter()
        .any(|item| same_value_zero(item, value))
}

fn for_each_live<F>(data: &Rc<SetData>, visit: &mut F) -> Result<(), VmError>
where
    F: FnMut(&Value) -> Result<bool, VmError>,
{
    let mut index = 0;
    loop {
        let Some(value) = data.values.borrow().get(index).cloned() else {
            break;
        };
        if !visit(&value)? {
            break;
        }
        let still_at_index = data
            .values
            .borrow()
            .get(index)
            .is_some_and(|current| same_value_zero(current, &value));
        if still_at_index {
            index += 1;
        }
    }
    Ok(())
}

fn fold_keys<T, F>(other: &SetRecord, init: T, step: &mut F) -> Result<T, VmError>
where
    F: FnMut(T, &Value) -> Result<Fold<T>, VmError>,
{
    match other {
        SetRecord::Native { data } => {
            let mut acc = init;
            let values: Vec<Value> = data.values.borrow().iter().cloned().collect();
            for value in values {
                acc = match step(acc, &value)? {
                    Fold::Next(acc) => acc,
                    Fold::Stop(acc) => return Ok(acc),
                };
            }
            Ok(acc)
        }
        SetRecord::Like { receiver, keys, .. } => fold_like_keys(receiver, keys, init, step),
    }
}

fn fold_like_keys<T, F>(
    receiver: &Value,
    keys: &Value,
    init: T,
    step: &mut F,
) -> Result<T, VmError>
where
    F: FnMut(T, &Value) -> Result<Fold<T>, VmError>,
{
    let object = crate::functions::execute_target(keys, receiver, &[])?;
    let iterator = crate::collections::iterator::open_self_iterator(object)?;
    let mut acc = init;
    loop {
        match crate::collections::iterator::step_value(&iterator) {
            Ok(Some(value)) => match step(acc, &value) {
                Ok(Fold::Next(next)) => acc = next,
                Ok(Fold::Stop(done)) => {
                    close_iterator(&iterator, crate::completion::Completion::Normal)?;
                    return Ok(done);
                }
                Err(error) => return close_with_error(&iterator, error),
            },
            Ok(None) => return Ok(acc),
            Err(error) => return close_with_error(&iterator, error),
        }
    }
}

fn close_with_error<T>(iterator: &Value, error: VmError) -> Result<T, VmError> {
    if let VmError::Thrown(reason) = &error {
        let _ = close_iterator(
            iterator,
            crate::completion::Completion::Throw(reason.clone()),
        );
    }
    Err(error)
}

fn close_iterator(
    iterator: &Value,
    completion: crate::completion::Completion,
) -> Result<(), VmError> {
    crate::collections::iterator::close(iterator.clone(), completion)?;
    Ok(())
}

fn new_set(values: Vec<Value>) -> Value {
    Value::Set(Rc::new(SetData {
        weak: false,
        values: std::cell::RefCell::new(
            values
                .into_iter()
                .map(|value| match value {
                    Value::Number(0.0) => Value::Number(0.0),
                    value => value,
                })
                .collect(),
        ),
        prototype: std::cell::RefCell::new(None),
    }))
}
