pub(crate) fn initialize_instance_fields_impl(
    function: &crate::value::FunctionValue,
    mut receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    for field in function.instance_fields.borrow().iter() {
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
    let value = match &field.initializer {
        crate::value::InstanceFieldInitializer::Undefined => Value::Undefined,
        crate::value::InstanceFieldInitializer::Callable(initializer) => {
            crate::functions::execute(initializer, &receiver, &[])?
        }
        crate::value::InstanceFieldInitializer::Value(value) => value.clone(),
        crate::value::InstanceFieldInitializer::PrivateMethod(value) => {
            return define_private_method(function, field, receiver, value.clone());
        }
        crate::value::InstanceFieldInitializer::PrivateAccessor { get, set } => {
            return define_private_accessor(function, field, receiver, get.clone(), set.clone());
        }
    };
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

fn constructor_receiver(target: &crate::value::Value) -> crate::value::Value {
    let prototype = crate::execute::get_property(target, "prototype");
    let prototype = if crate::value::is_object(&prototype) {
        prototype
    } else {
        constructor_receiver_default(target)
    };
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("\0prototype".to_string(), prototype),
    ])))
}

fn constructor_receiver_default(target: &crate::value::Value) -> crate::value::Value {
    if let crate::value::Value::Function(function) = target {
        let global = function.captures.get(0);
        let object_constructor = crate::execute::get_property(&global, "Object");
        let object_prototype = crate::execute::get_property(&object_constructor, "prototype");
        if crate::value::is_object(&object_prototype) {
            return object_prototype;
        }
    }
    crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)
}

fn builtin_default_prototype(target: &crate::value::Value) -> Option<crate::value::Value> {
    let crate::value::Value::Builtin(builtin) = target else {
        return None;
    };
    crate::builtin_meta::prototype(*builtin).map(crate::value::Value::Builtin)
}
