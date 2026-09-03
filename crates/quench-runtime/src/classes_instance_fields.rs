pub(crate) fn append_instance_field(
    registers: &crate::register_file::RegisterFile,
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

pub(crate) fn execute_static_block(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Op::StaticBlock {
        constructor,
        captures,
        body,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let function = Op::MakeFunctionWithKind {
        dst: 0,
        body: body.clone(),
        params: 0,
        captures: *captures,
        length: 0,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Strict,
        is_async: false,
        mapped_arguments: false,
        source: None,
    };
    let receiver = crate::execute::read_register(registers, *constructor)?;
    let mut block_registers = crate::register_file::RegisterFile::new();
    crate::functions::write_op(&mut block_registers, &function);
    let block = crate::execute::read_register(&block_registers, 0)?;
    crate::super_scope::attach_home(&block, &receiver);
    crate::functions::execute_target(&block, &receiver, &[])?;
    Ok(crate::completion::Completion::Normal)
}
include!("instance_fields_private.rs");

type PrivateAccessorValues =
    Option<(Option<crate::value::Value>, Option<crate::value::Value>)>;

fn private_accessor_values(
    registers: &crate::register_file::RegisterFile,
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
    registers: &crate::register_file::RegisterFile,
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
    registers: &crate::register_file::RegisterFile,
    field: &AppendInstanceFieldOp,
) -> Result<(), crate::execute::VmError> {
    let constructor = crate::execute::read_register(registers, field.constructor)?;
    let key = field_key_value(registers, &field.key)?;
    let initializer = instance_field_initializer(field.initializer.as_ref())?;
    if let crate::value::InstanceFieldInitializer::Callable(function) = &initializer {
        crate::super_scope::attach_home(
            &crate::value::Value::Function(std::rc::Rc::clone(function)),
            &constructor,
        );
    }
    let value = field_initializer_value(&initializer, &constructor)?;
    let value = name_static_field(&field.key, value)?;
    define_public_field(&constructor, &key, value)?;
    Ok(())
}
fn field_key_value(
    registers: &crate::register_file::RegisterFile,
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
            crate::functions::execute_target(
                &crate::value::Value::Function(std::rc::Rc::clone(function)),
                receiver,
                &[],
            )
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
    registers: &crate::register_file::RegisterFile,
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
            direct_constructor: std::rc::Rc::default(),
            composed_constructor: std::rc::Rc::default(),
        },
    );
    let crate::value::Value::Function(function) = value else {
        return Err(crate::execute::VmError::NotCallable);
    };
    Ok(crate::value::InstanceFieldInitializer::Callable(function))
}

fn name_static_field(
    key: &InstanceFieldKeyOp,
    value: crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let name = match key {
        InstanceFieldKeyOp::Static(name) => name.as_str(),
        InstanceFieldKeyOp::Private(_) | InstanceFieldKeyOp::Dynamic(_) => return Ok(value),
    };
    if crate::execute::get_property(&value, "name") == crate::value::Value::String(String::new()) {
        let _ = crate::builtins::set_function_name(&value, name);
    }
    Ok(value)
}

fn name_initialized_value(
    initializer: Option<&InstanceFieldInitializerOp>,
    value: crate::value::Value,
) -> crate::value::Value {
    let Some(name) = initializer.and_then(|init| init.name.as_deref()) else {
        return value;
    };
    if crate::execute::get_property(&value, "name") == crate::value::Value::String(String::new()) {
        let _ = crate::builtins::set_function_name(&value, name);
    }
    value
}
