use std::collections::HashMap;

use oxc::ast::ast::{Class, ClassElement, MethodDefinition, MethodDefinitionKind, PropertyKey};

use crate::{
    facts::ProgramDb,
    ops::{Constant, FunctionKind, FunctionStrictness, Op, PropertyDefinitionKind},
};

pub(crate) fn reduce_expression(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let constructor = reduce_constructor(class, ops, facts, next, locals)?;
    let prototype = emit_object(ops, next, Vec::new());
    define_static_key(
        ops,
        next,
        prototype,
        "constructor",
        constructor,
        PropertyDefinitionKind::Data,
    );
    reduce_instance_methods(class, prototype, ops, facts, next, locals)?;
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
    prototype: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    for element in &class.body.body {
        let ClassElement::MethodDefinition(method) = element else {
            continue;
        };
        if method.r#static || method.kind == MethodDefinitionKind::Constructor {
            continue;
        }
        let key = reduce_method_key(method, ops, facts, next, locals)?;
        let value = reduce_method(method, ops, facts, next, locals)?;
        define_method(ops, prototype, key, value, method.kind);
    }
    Some(())
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
        let key = reduce_method_key(method, ops, facts, next, locals)?;
        let value = reduce_method(method, ops, facts, next, locals)?;
        define_method(ops, constructor, key, value, method.kind);
    }
    Some(())
}

fn define_method(ops: &mut Vec<Op>, object: u16, key: u16, value: u16, kind: MethodDefinitionKind) {
    ops.push(Op::DefineProperty {
        object,
        key,
        value,
        kind: property_kind(kind),
        enumerable: false,
    });
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

fn reduce_method_key(
    method: &MethodDefinition<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if method.computed {
        return crate::reduce::reduce_expression(
            method.key.as_expression()?,
            ops,
            facts,
            next,
            locals,
        );
    }
    let key = method_key(&method.key)?;
    Some(emit_string(ops, next, key))
}

fn property_kind(kind: MethodDefinitionKind) -> PropertyDefinitionKind {
    match kind {
        MethodDefinitionKind::Get => PropertyDefinitionKind::Get,
        MethodDefinitionKind::Set => PropertyDefinitionKind::Set,
        _ => PropertyDefinitionKind::Data,
    }
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

fn emit_string(ops: &mut Vec<Op>, next: &mut u16, value: String) -> u16 {
    let dst = take_register(next);
    ops.push(Op::Const {
        dst,
        value: Constant::String(value),
    });
    dst
}

fn define_static_key(
    ops: &mut Vec<Op>,
    next: &mut u16,
    object: u16,
    key: &str,
    value: u16,
    kind: PropertyDefinitionKind,
) {
    let key = emit_string(ops, next, key.to_string());
    ops.push(Op::DefineProperty {
        object,
        key,
        value,
        kind,
        enumerable: false,
    });
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
