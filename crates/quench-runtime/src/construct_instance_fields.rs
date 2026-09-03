pub(crate) fn initialize_instance_fields_impl(
    function: &crate::value::FunctionValue,
    mut receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    receiver = crate::locals::resolved_replacement(receiver);
    let fields = function.instance_fields.borrow();
    for field in fields.iter().filter(|field| is_private_method(field)) {
        let previous = receiver.clone();
        receiver = initialize_instance_field(function, field, receiver)?;
        crate::locals::replace_value(&previous, &receiver);
    }
    for field in fields.iter().filter(|field| !is_private_method(field)) {
        let previous = receiver.clone();
        receiver = initialize_instance_field(function, field, receiver)?;
        crate::locals::replace_value(&previous, &receiver);
    }
    Ok(receiver)
}

fn initialize_instance_field(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    let Some(value) = field_initializer_value(function, field, &receiver)? else {
        return Ok(receiver);
    };
    define_initialized_field(function, field, receiver, value)
}

fn field_initializer_value(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
    receiver: &Value,
) -> Result<Option<Value>, crate::execute::VmError> {
    match &field.initializer {
        crate::value::InstanceFieldInitializer::Undefined => Ok(Some(Value::Undefined)),
        crate::value::InstanceFieldInitializer::Callable(initializer) => {
            crate::super_scope::attach_home(
                &Value::Function(std::rc::Rc::clone(initializer)),
                receiver,
            );
            let value = crate::functions::execute_target(
                &Value::Function(std::rc::Rc::clone(initializer)),
                receiver,
                &[],
            )?;
            name_field_value(&field.key, value).map(Some)
        }
        crate::value::InstanceFieldInitializer::Value(value) => Ok(Some(value.clone())),
        crate::value::InstanceFieldInitializer::PrivateMethod(value) => {
            define_private_method(function, field, receiver.clone(), value.clone())?;
            Ok(None)
        }
        crate::value::InstanceFieldInitializer::PrivateAccessor { get, set } => {
            define_private_accessor(function, field, receiver.clone(), get.clone(), set.clone())?;
            Ok(None)
        }
    }
}

fn define_initialized_field(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
    receiver: Value,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let receiver = crate::locals::resolved_replacement(receiver);
    match &field.key {
        crate::value::InstanceFieldKey::Private(id) => {
            let name = function.private_environment.resolve(*id).ok_or_else(|| {
                crate::value::error::throw_type_error(
                    "Private field access on an object without the required brand",
                )
            })?;
            crate::private_slots::define(&receiver, name, value)?;
            Ok(receiver)
        }
        crate::value::InstanceFieldKey::Static(key) => {
            define_instance_field(receiver, key.as_ref(), value)
        }
        crate::value::InstanceFieldKey::Dynamic(key) => {
            let key = crate::conversion::to_property_key(key)?;
            define_instance_field(receiver, &key, value)
        }
    }
}

fn define_private_method(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
    receiver: Value,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let name = private_field_name(function, field)?;
    crate::private_slots::define_method(&receiver, name, value)?;
    Ok(receiver)
}

fn define_private_accessor(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
    receiver: Value,
    get: Option<Value>,
    set: Option<Value>,
) -> Result<Value, crate::execute::VmError> {
    let name = private_field_name(function, field)?;
    crate::private_slots::define_accessor(&receiver, name, get, set)?;
    Ok(receiver)
}

fn private_field_name(
    function: &crate::value::FunctionValue,
    field: &crate::value::InstanceFieldPlan,
) -> Result<crate::value::PrivateName, crate::execute::VmError> {
    let crate::value::InstanceFieldKey::Private(id) = field.key else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    function.private_environment.resolve(id).ok_or_else(|| {
        crate::value::error::throw_type_error(
            "Private field access on an object without the required brand",
        )
    })
}

fn define_instance_field(
    receiver: Value,
    key: &str,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let descriptor = [
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    crate::builtins::define_own_property(&receiver, key, &descriptor)
}

fn is_private_method(field: &crate::value::InstanceFieldPlan) -> bool {
    matches!(
        field.initializer,
        crate::value::InstanceFieldInitializer::PrivateMethod(_)
            | crate::value::InstanceFieldInitializer::PrivateAccessor { .. }
    )
}

fn name_field_value(
    key: &crate::value::InstanceFieldKey,
    value: crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let name = match key {
        crate::value::InstanceFieldKey::Static(name) => name.as_ref(),
        crate::value::InstanceFieldKey::Private(_) => {
            return Ok(value);
        }
        crate::value::InstanceFieldKey::Dynamic(_) => return Ok(value),
    };
    if crate::execute::get_property(&value, "name") == crate::value::Value::String(String::new()) {
        let _ = crate::builtins::set_function_name(&value, name);
    }
    Ok(value)
}

fn constructor_receiver(target: &crate::value::Value) -> crate::value::Value {
    let target = crate::construct::peel_construct_value(target);
    let target = match &target {
        crate::value::Value::BoundFunction(bound) => {
            crate::construct::peel_construct_value(&bound.target)
        }
        _ => target,
    };
    let prototype = crate::construct::get_prototype_from_constructor(&target, |realm| {
        crate::vm::realm_intrinsic_for(realm, crate::ops::Builtin::ObjectPrototype)
    });
    let object = std::rc::Rc::new(crate::value::ObjectData::new(vec![(
        "\0prototype".to_string(),
        prototype.clone(),
    )]));
    object.capture_original_prototype(prototype);
    let value = crate::value::Value::Object(object);
    value
}

fn builtin_default_prototype(target: &crate::value::Value) -> Option<crate::value::Value> {
    let crate::value::Value::Builtin(builtin) = target else {
        return None;
    };
    let prototype = crate::builtin_meta::instance_prototype(*builtin)?;
    Some(crate::value::Value::Builtin(prototype))
}
