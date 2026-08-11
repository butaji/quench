pub(crate) fn define_properties(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    let Some(Value::Object(properties)) = arguments.get(1) else {
        return Err(crate::value::error::throw_type_error(
            "Property descriptors must be an object",
        ));
    };
    let descriptors = Value::Object(std::rc::Rc::clone(properties));
    properties
        .iter()
        .filter(|(key, _)| !key.starts_with('\0') && !is_descriptor_key(key))
        .try_fold(target, |target, (key, _)| {
            let descriptor = crate::execute::get_property_result(&descriptors, key)?;
            let Some(descriptor) = descriptor_object(descriptor) else {
                return Err(crate::value::error::throw_type_error(
                    "Property descriptor must be an object",
                ));
            };
            define_own_property(&target, key, &descriptor)
        })
}

fn descriptor_object(value: Value) -> Option<std::rc::Rc<crate::value::ObjectData>> {
    match value {
        Value::Object(properties) => Some(properties),
        Value::ObjectAlias(alias) => alias.0.borrow().upgrade(),
        _ => None,
    }
}
