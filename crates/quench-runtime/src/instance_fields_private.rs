fn append_private_field(
    registers: &[crate::value::Value],
    field: &AppendInstanceFieldOp,
    id: crate::facts::PrivateNameId,
) -> Result<(), crate::execute::VmError> {
    let constructor = crate::execute::read_register(registers, field.constructor)?;
    let crate::value::Value::Function(_function) = &constructor else {
        return Err(crate::execute::VmError::NotCallable);
    };
    if field.is_static {
        return define_static_private_field(registers, field, constructor, id);
    }
    define_instance_private_field(registers, field, &constructor, id)
}

fn define_static_private_field(
    registers: &[crate::value::Value],
    field: &AppendInstanceFieldOp,
    constructor: crate::value::Value,
    id: crate::facts::PrivateNameId,
) -> Result<(), crate::execute::VmError> {
    let name = crate::private_environment::resolve(id).ok_or_else(|| {
        crate::value::error::throw_type_error(
            "Private field access on an object without the required brand",
        )
    })?;
    if let Some(accessor) = private_accessor_values(registers, field)? {
        return crate::private_slots::define_accessor(&constructor, name, accessor.0, accessor.1);
    }
    let value = private_field_value(registers, field, &constructor)?;
    if field.value.is_some() {
        return crate::private_slots::define_method(&constructor, name, value);
    }
    crate::private_slots::define(&constructor, name, value)
}

fn define_instance_private_field(
    registers: &[crate::value::Value],
    field: &AppendInstanceFieldOp,
    function: &crate::value::Value,
    id: crate::facts::PrivateNameId,
) -> Result<(), crate::execute::VmError> {
    let initializer = match private_accessor_values(registers, field)? {
        Some((get, set)) => crate::value::InstanceFieldInitializer::PrivateAccessor { get, set },
        None => match field.value {
            Some(value) => {
                crate::value::InstanceFieldInitializer::PrivateMethod(
                    crate::execute::read_register(registers, value)?,
                )
            }
            None => instance_field_initializer(field.initializer.as_ref())?,
        },
    };
    let crate::value::Value::Function(function) = function else {
        return Err(crate::execute::VmError::NotCallable);
    };
    function
        .instance_fields
        .borrow_mut()
        .push(crate::value::InstanceFieldPlan {
            key: crate::value::InstanceFieldKey::Private(id),
            initializer,
        });
    Ok(())
}
