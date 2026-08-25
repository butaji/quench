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
        let realm = crate::vm::realm_id_for_global_value(&function.captures.get(0));
        let constructor = crate::value::Value::WeakFunction(crate::value::WeakFunctionValue(
            std::rc::Rc::downgrade(function),
        ));
        let prototype = realm
            .and_then(|realm| {
                crate::vm::with_realm(realm, || {
                    Some(crate::value::Value::Object(std::rc::Rc::new(
                        crate::value::ObjectData::new(vec![
                            (
                                "\0prototype".to_string(),
                                crate::vm::realm_intrinsic(crate::ops::Builtin::ObjectPrototype),
                            ),
                            ("constructor".to_string(), constructor.clone()),
                            (
                                crate::builtins::descriptor_key("constructor"),
                                constructor_descriptor(constructor.clone()),
                            ),
                        ]),
                    )))
                })
            })
            .flatten()
            .unwrap_or_else(|| {
                crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
                    vec![
                        (
                            "\0prototype".to_string(),
                            crate::vm::realm_intrinsic(crate::ops::Builtin::ObjectPrototype),
                        ),
                        ("constructor".to_string(), constructor.clone()),
                        (
                            crate::builtins::descriptor_key("constructor"),
                            constructor_descriptor(constructor.clone()),
                        ),
                    ],
                )))
            });
        function.properties.borrow_mut().push(("prototype".to_string(), prototype));
    }
}

fn constructor_descriptor(value: crate::value::Value) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        (
            "enumerable".to_string(),
            crate::value::Value::Boolean(false),
        ),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(true),
        ),
    ])))
}

fn attach_generator_prototype(function: &std::rc::Rc<crate::value::FunctionValue>) {
    let parent = if function.is_async {
        crate::builtins::async_generator_prototype()
    } else {
        crate::builtins::generator_prototype()
    };
    let instance = crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
        vec![("\0prototype".to_string(), parent)],
    )));
    let function_prototype = crate::vm::realm_intrinsic(if function.is_async {
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
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    body: &crate::machine::FunctionCode,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
) {
    let captures = crate::environment::Environment::capture(&crate::locals::current(), captures);
    let value = make(
        body.clone(),
        params,
        metadata.length,
        captures,
        metadata,
    );
    if matches!(metadata.kind, FunctionKind::Ordinary) {
        if let crate::value::Value::Function(function) = &value {
            function.properties.borrow_mut().push((
                "\0ordinary_function".to_string(),
                crate::value::Value::Boolean(true),
            ));
            let mut prototype = crate::value::Value::Object(std::rc::Rc::new(
                crate::value::ObjectData::new(vec![(
                    "\0prototype".to_string().into(),
                    crate::vm::realm_intrinsic(crate::ops::Builtin::ObjectPrototype),
                )]),
            ));
            if let crate::value::Value::Object(object) = &mut prototype {
                std::rc::Rc::get_mut(object)
                    .expect("ordinary function prototype is uniquely owned")
                    .properties
                    .push(("constructor".to_string().into(), value.clone()));
            }
            let mut properties = function.properties.borrow_mut();
            properties.push(("prototype".to_string(), prototype.clone()));
            properties.push((
                crate::builtins::descriptor_key("prototype"),
                crate::value::Value::Object(std::rc::Rc::new(
                    crate::value::ObjectData::new(vec![
                        ("value".to_string().into(), prototype),
                        ("writable".to_string().into(), crate::value::Value::Boolean(true)),
                        ("enumerable".to_string().into(), crate::value::Value::Boolean(false)),
                        ("configurable".to_string().into(), crate::value::Value::Boolean(false)),
                    ]),
                )),
            ));
        }
    }
    crate::execute::write_value(registers, dst, value);
}

pub(crate) fn write_op(registers: &mut crate::register_file::RegisterFile, op: &Op) {
    crate::functions_write::write_op(registers, op);
}
