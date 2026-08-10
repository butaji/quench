use std::collections::HashMap;

use oxc::ast::ast::{Class, ClassElement, MethodDefinition, MethodDefinitionKind, PropertyKey};

use crate::{
    facts::ProgramDb,
    ops::{Constant, FunctionKind, FunctionStrictness, Op},
};

pub(crate) fn reduce_expression(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let constructor = reduce_constructor(class, ops, facts, next, locals)?;
    let mut properties = reduce_instance_methods(class, ops, facts, next, locals)?;
    properties.push(("constructor".to_string(), constructor));
    let prototype = emit_object(ops, next, properties);
    ops.push(Op::SetProperty {
        object: constructor,
        key: "prototype".to_string(),
        src: prototype,
    });
    reduce_static_methods(class, constructor, ops, facts, next, locals)?;
    Some(constructor)
}

fn reduce_constructor(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let method = class.body.body.iter().find_map(|element| match element {
        ClassElement::MethodDefinition(method)
            if method.kind == MethodDefinitionKind::Constructor =>
        {
            Some(method.as_ref())
        }
        _ => None,
    });
    match method {
        Some(method) => reduce_method(method, ops, facts, next, locals),
        None => Some(emit_default_constructor(ops, next)),
    }
}

fn reduce_instance_methods(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Vec<(String, u16)>> {
    let mut properties = Vec::new();
    for element in &class.body.body {
        let ClassElement::MethodDefinition(method) = element else {
            continue;
        };
        if method.r#static || method.kind == MethodDefinitionKind::Constructor {
            continue;
        }
        let key = method_key(&method.key)?;
        let value = reduce_method(method, ops, facts, next, locals)?;
        push_method_property(&mut properties, method, key, value, ops, next);
    }
    Some(properties)
}

fn reduce_static_methods(
    class: &Class<'_>,
    constructor: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    for element in &class.body.body {
        let ClassElement::MethodDefinition(method) = element else {
            continue;
        };
        if !method.r#static {
            continue;
        }
        let key = method_key(&method.key)?;
        let value = reduce_method(method, ops, facts, next, locals)?;
        set_static_method(ops, next, constructor, method, key, value);
    }
    Some(())
}

fn set_static_method(
    ops: &mut Vec<Op>,
    next: &mut u16,
    constructor: u16,
    method: &MethodDefinition<'_>,
    key: String,
    value: u16,
) {
    if method.kind == MethodDefinitionKind::Get {
        let descriptor = emit_getter_descriptor(ops, next, value);
        let undefined = emit_undefined(ops, next);
        set_property(ops, constructor, key.clone(), undefined);
        set_property(
            ops,
            constructor,
            crate::builtins::descriptor_key(&key),
            descriptor,
        );
    } else {
        set_property(ops, constructor, key, value);
    }
}

fn set_property(ops: &mut Vec<Op>, object: u16, key: String, src: u16) {
    ops.push(Op::SetProperty { object, key, src });
}

fn reduce_method(
    method: &MethodDefinition<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let inherited = facts.strict;
    facts.strict = true;
    let result = crate::functions::reduce_expression(&method.value, ops, facts, next, locals);
    facts.strict = inherited;
    result
}

fn push_method_property(
    properties: &mut Vec<(String, u16)>,
    method: &MethodDefinition<'_>,
    key: String,
    value: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
) {
    if method.kind == MethodDefinitionKind::Get {
        let descriptor = emit_getter_descriptor(ops, next, value);
        properties.push((key.clone(), emit_undefined(ops, next)));
        properties.push((crate::builtins::descriptor_key(&key), descriptor));
    } else {
        properties.push((key, value));
    }
}

fn emit_getter_descriptor(ops: &mut Vec<Op>, next: &mut u16, getter: u16) -> u16 {
    let false_value = emit_boolean(ops, next, false);
    let true_value = emit_boolean(ops, next, true);
    emit_object(
        ops,
        next,
        vec![
            ("get".to_string(), getter),
            ("enumerable".to_string(), false_value),
            ("configurable".to_string(), true_value),
        ],
    )
}

fn emit_default_constructor(ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let dst = take_register(next);
    ops.push(Op::MakeFunctionWithKind {
        dst,
        body: vec![
            Op::Const {
                dst: 0,
                value: Constant::Undefined,
            },
            Op::Return { src: 0 },
        ],
        params: 0,
        captures: 0,
        kind: FunctionKind::Ordinary,
        strictness: FunctionStrictness::Strict,
        is_async: false,
        mapped_arguments: false,
    });
    dst
}

fn emit_object(ops: &mut Vec<Op>, next: &mut u16, properties: Vec<(String, u16)>) -> u16 {
    let dst = take_register(next);
    ops.push(Op::MakeObject { dst, properties });
    dst
}

fn emit_undefined(ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let dst = take_register(next);
    ops.push(Op::Const {
        dst,
        value: Constant::Undefined,
    });
    dst
}

fn emit_boolean(ops: &mut Vec<Op>, next: &mut u16, value: bool) -> u16 {
    let dst = take_register(next);
    ops.push(Op::Const {
        dst,
        value: Constant::Boolean(value),
    });
    dst
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}

fn method_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(key) => Some(key.name.to_string()),
        PropertyKey::PrivateIdentifier(key) => Some(format!("#{}", key.name)),
        PropertyKey::StringLiteral(key) => Some(key.value.to_string()),
        PropertyKey::NumericLiteral(key) => Some(key.value.to_string()),
        _ => None,
    }
}
