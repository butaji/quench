pub(crate) fn append_instance_field(
    registers: &[crate::value::Value],
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::AppendInstanceField(field) = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    if let InstanceFieldKeyOp::Private(id) = &field.key {
        return append_private_field(registers, field, *id);
    }
    if field.is_static {
        return define_static_field(registers, field);
    }
    let constructor = crate::execute::read_register(registers, field.constructor)?;
    let crate::value::Value::Function(constructor) = constructor else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let key = instance_field_key(registers, &field.key)?;
    let initializer = instance_field_initializer(field.initializer.as_ref())?;
    constructor
        .instance_fields
        .borrow_mut()
        .push(crate::value::InstanceFieldPlan { key, initializer });
    Ok(())
}
include!("instance_fields_private.rs");

type PrivateAccessorValues =
    Option<(Option<crate::value::Value>, Option<crate::value::Value>)>;

fn private_accessor_values(
    registers: &[crate::value::Value],
    field: &AppendInstanceFieldOp,
) -> Result<PrivateAccessorValues, crate::execute::VmError> {
    let Some(accessor) = &field.accessor else {
        return Ok(None);
    };
    let get = accessor.get.map(|index| crate::execute::read_register(registers, index)).transpose()?;
    let set = accessor.set.map(|index| crate::execute::read_register(registers, index)).transpose()?;
    Ok(Some((get, set)))
}
fn private_field_value(
    registers: &[crate::value::Value],
    field: &AppendInstanceFieldOp,
    receiver: &crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    match field.value {
        Some(value) => crate::execute::read_register(registers, value),
        None => {
            let initializer = instance_field_initializer(field.initializer.as_ref())?;
            field_initializer_value(&initializer, receiver)
        }
    }
}
fn define_static_field(
    registers: &[crate::value::Value],
    field: &AppendInstanceFieldOp,
) -> Result<(), crate::execute::VmError> {
    let constructor = crate::execute::read_register(registers, field.constructor)?;
    let key = field_key_value(registers, &field.key)?;
    let initializer = instance_field_initializer(field.initializer.as_ref())?;
    let value = field_initializer_value(&initializer, &constructor)?;
    define_public_field(&constructor, &key, value)?;
    Ok(())
}
fn field_key_value(
    registers: &[crate::value::Value],
    key: &InstanceFieldKeyOp,
) -> Result<String, crate::execute::VmError> {
    match key {
        InstanceFieldKeyOp::Static(key) => Ok(key.clone()),
        InstanceFieldKeyOp::Dynamic(src) => {
            crate::conversion::to_property_key(&crate::execute::read_register(registers, *src)?)
        }
        InstanceFieldKeyOp::Private(_) => Err(crate::execute::VmError::MissingReturn),
    }
}

fn field_initializer_value(
    initializer: &crate::value::InstanceFieldInitializer,
    receiver: &crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    match initializer {
        crate::value::InstanceFieldInitializer::Undefined => Ok(crate::value::Value::Undefined),
        crate::value::InstanceFieldInitializer::Callable(function) => {
            crate::functions::execute(function, receiver, &[])
        }
        crate::value::InstanceFieldInitializer::Value(value) => Ok(value.clone()),
        crate::value::InstanceFieldInitializer::PrivateMethod(value) => Ok(value.clone()),
        crate::value::InstanceFieldInitializer::PrivateAccessor { .. } => {
            Err(crate::execute::VmError::MissingReturn)
        }
    }
}

fn define_public_field(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let descriptor = [
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        ("enumerable".to_string(), crate::value::Value::Boolean(true)),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(true),
        ),
    ];
    crate::builtins::define_own_property(target, key, &descriptor)
}

fn instance_field_key(
    registers: &[crate::value::Value],
    key: &InstanceFieldKeyOp,
) -> Result<crate::value::InstanceFieldKey, crate::execute::VmError> {
    Ok(match key {
        InstanceFieldKeyOp::Static(key) => {
            crate::value::InstanceFieldKey::Static(std::rc::Rc::from(key.as_str()))
        }
        InstanceFieldKeyOp::Dynamic(src) => {
            crate::value::InstanceFieldKey::Dynamic(crate::execute::read_register(registers, *src)?)
        }
        InstanceFieldKeyOp::Private(id) => crate::value::InstanceFieldKey::Private(*id),
    })
}

fn instance_field_initializer(
    initializer: Option<&InstanceFieldInitializerOp>,
) -> Result<crate::value::InstanceFieldInitializer, crate::execute::VmError> {
    let Some(initializer) = initializer else {
        return Ok(crate::value::InstanceFieldInitializer::Undefined);
    };
    let value = crate::functions::make(
        initializer.body.clone(),
        0,
        0,
        crate::locals::capture(initializer.captures),
        crate::functions::FunctionMetadata {
            kind: FunctionKind::Ordinary,
            length: 0,
            strictness: FunctionStrictness::Strict,
            is_async: false,
            mapped_arguments: false,
        },
    );
    let crate::value::Value::Function(function) = value else {
        return Err(crate::execute::VmError::NotCallable);
    };
    Ok(crate::value::InstanceFieldInitializer::Callable(function))
}
