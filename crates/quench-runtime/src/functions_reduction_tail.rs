fn attach_lexical_super(value: &crate::value::Value, kind: FunctionKind) {
    if !matches!(kind, FunctionKind::Arrow) {
        return;
    }
    let Some((home, receiver, lexical_function)) = crate::super_scope::capture_lexical() else {
        return;
    };
    let crate::value::Value::Function(function) = value else {
        return;
    };
    let mut properties = function.properties.borrow_mut();
    properties.push(("\0home_object".to_string(), home));
    properties.push(("\0super_receiver".to_string(), receiver));
    properties.push(("\0super_function".to_string(), lexical_function));
}

fn attach_prototype(value: &crate::value::Value) {
    if let crate::value::Value::Function(function) = value {
        if function.kind == FunctionKind::Generator {
            return attach_generator_prototype(function);
        }
    }
    let prototype = crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
        vec![("constructor".to_string(), value.clone())],
    )));
    if let crate::value::Value::Function(function) = value {
        function
            .properties
            .borrow_mut()
            .push(("prototype".to_string(), prototype));
    }
}

fn attach_generator_prototype(function: &std::rc::Rc<crate::value::FunctionValue>) {
    let parent = if function.is_async {
        crate::builtins::async_generator_prototype()
    } else {
        crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)
    };
    let instance = crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
        vec![("\0prototype".to_string(), parent)],
    )));
    let function_prototype = crate::value::Value::Builtin(if function.is_async {
        crate::ops::Builtin::AsyncGeneratorFunctionPrototype
    } else {
        crate::ops::Builtin::GeneratorFunctionPrototype
    });
    function.properties.borrow_mut().extend([
        ("\0prototype".to_string(), function_prototype),
        ("prototype".to_string(), instance.clone()),
        (
            crate::builtins::descriptor_key("prototype"),
            prototype_descriptor(instance),
        ),
    ]);
}

fn prototype_descriptor(value: crate::value::Value) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        (
            "enumerable".to_string(),
            crate::value::Value::Boolean(false),
        ),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(false),
        ),
    ])))
}

pub(crate) fn write(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    body: &crate::machine::FunctionCode,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
) {
    let value = make(
        body.clone(),
        params,
        metadata.length,
        crate::locals::capture(captures),
        metadata,
    );
    if matches!(metadata.kind, FunctionKind::Ordinary) {
        if let crate::value::Value::Function(function) = &value {
            function.properties.borrow_mut().push((
                "\0ordinary_function".to_string(),
                crate::value::Value::Boolean(true),
            ));
        }
    }
    crate::execute::write_value(registers, dst, value);
}

pub(crate) fn write_op(registers: &mut Vec<crate::value::Value>, op: &Op) {
    crate::functions_write::write_op(registers, op);
}
