use crate::{execute::VmError, ops::PropertyDefinitionKind, value::Value};

pub(crate) fn execute(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    let crate::ops::Op::DefineProperty {
        object,
        key,
        value,
        kind,
        enumerable,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?;
    let key = crate::conversion::to_property_key(&crate::execute::read_register(registers, *key)?)?;
    let value = crate::execute::read_register(registers, *value)?;
    let descriptor = descriptor(*kind, value, *enumerable);
    let result = crate::builtins::define_own_property(&target, &key, &descriptor)?;
    crate::locals::replace_value(&target, &result);
    crate::vm::synchronize_global_object(registers, &target, &result);
    crate::execute::write_value(registers, *object, result);
    Ok(())
}

fn descriptor(
    kind: PropertyDefinitionKind,
    value: Value,
    enumerable: bool,
) -> Vec<(String, Value)> {
    let mut fields = match kind {
        PropertyDefinitionKind::Data => vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
        ],
        PropertyDefinitionKind::Get => vec![("get".to_string(), value)],
        PropertyDefinitionKind::Set => vec![("set".to_string(), value)],
    };
    fields.push(("enumerable".to_string(), Value::Boolean(enumerable)));
    fields.push(("configurable".to_string(), Value::Boolean(true)));
    fields
}

pub(crate) fn accessor(value: &Value, key: &str, field: &str) -> Option<Value> {
    if let Some(value) = crate::vm::array_accessor(value, key, field) {
        return Some(value);
    }
    let key = crate::builtins::descriptor_key(key);
    let properties = match value {
        Value::Object(properties) => properties.as_slice(),
        Value::Function(function) => {
            return accessor_field(&function.properties.borrow(), &key, field)
        }
        _ => return None,
    };
    accessor_field(properties, &key, field)
}

fn accessor_field(properties: &[(String, Value)], key: &str, field: &str) -> Option<Value> {
    let descriptor = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value));
    if let Some(Value::Object(descriptor)) = descriptor {
        return descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == field).then(|| value.clone()));
    }
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value));
    match prototype {
        Some(Value::Object(properties)) => accessor_field(properties, key, field),
        _ => None,
    }
}
