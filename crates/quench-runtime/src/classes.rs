use std::collections::HashMap;

use oxc::ast::ast::{Class, ClassElement, MethodDefinition, MethodDefinitionKind, PropertyKey};

use crate::{
    facts::ProgramDb,
    ops::{
        AppendInstanceFieldOp, Constant, FunctionKind, FunctionStrictness,
        InstanceFieldInitializerOp, InstanceFieldKeyOp, Op, PropertyDefinitionKind,
    },
};

include!("classes_private_scope.rs");
include!("classes_name.rs");
include!("classes_method_name.rs");

fn reduce_heritage(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Option<u16>> {
    let Some(super_class) = &class.super_class else {
        return Some(None);
    };
    let src = crate::reduce::reduce_expression(super_class, ops, facts, next, locals)?;
    ops.push(Op::ValidateClassHeritage { src });
    Some(Some(src))
}
fn configure_heritage(
    heritage: Option<u16>,
    constructor: u16,
    prototype: u16,
    default_constructor: bool,
    ops: &mut Vec<Op>,
    next: &mut u16,
) {
    let Some(heritage) = heritage else { return };
    let parent_prototype = take_register(next);
    ops.push(Op::GetProperty {
        dst: parent_prototype,
        object: heritage,
        key: "prototype".to_string(),
    });
    set_internal_prototype(ops, prototype, parent_prototype);
    set_internal_prototype(ops, constructor, heritage);
    ops.push(Op::SetProperty {
        object: constructor,
        key: "\0derived_constructor".to_string(),
        src: heritage,
    });
    if default_constructor {
        ops.push(Op::SetProperty {
            object: constructor,
            key: "\0default_derived_constructor".to_string(),
            src: heritage,
        });
    }
}
fn set_internal_prototype(ops: &mut Vec<Op>, object: u16, src: u16) {
    ops.push(Op::SetProperty {
        object,
        key: "\0prototype".to_string(),
        src,
    });
}
pub(crate) fn validate_heritage(
    value: &crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    if matches!(value, crate::value::Value::Null) || crate::conversion::is_callable(value) {
        return Ok(());
    }
    Err(crate::value::error::throw_type_error(
        "Class extends value is not a constructor or null",
    ))
}
include!("classes_instance_fields.rs");

fn reduce_constructor(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, bool)> {
    let method = class.body.body.iter().find_map(|element| match element {
        ClassElement::MethodDefinition(method)
            if method.kind == MethodDefinitionKind::Constructor =>
        {
            Some(method.as_ref())
        }
        _ => None,
    });
    match method {
        Some(method) => reduce_method(method, ops, facts, next, locals).map(|value| (value, false)),
        None => Some((emit_default_constructor(ops, next), true)),
    }
}

fn reduce_elements(
    class: &Class<'_>,
    prototype: u16,
    constructor: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Vec<Op>> {
    let mut static_fields = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                reduce_class_method(method, prototype, constructor, ops, facts, next, locals)?
            }
            ClassElement::PropertyDefinition(field)
                if !matches!(field.key, PropertyKey::PrivateIdentifier(_)) =>
            {
                reduce_public_field(
                    field,
                    constructor,
                    ops,
                    &mut static_fields,
                    facts,
                    next,
                    locals,
                )?
            }
            ClassElement::PropertyDefinition(field)
                if matches!(field.key, PropertyKey::PrivateIdentifier(_)) =>
            {
                reduce_private_field(field, constructor, ops, &mut static_fields, facts, locals)?
            }
            _ => {}
        }
    }
    Some(static_fields)
}

fn reduce_class_method(
    method: &MethodDefinition<'_>,
    prototype: u16,
    constructor: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    if method.kind == MethodDefinitionKind::Constructor {
        return Some(());
    }
    if let PropertyKey::PrivateIdentifier(name) = &method.key {
        if !matches!(
            method.kind,
            MethodDefinitionKind::Get | MethodDefinitionKind::Set
        ) {
            return reduce_private_method(method, name, constructor, ops, facts, next, locals);
        }
    }
    let key = reduce_method_key(method, ops, facts, next, locals)?;
    let value = reduce_method(method, ops, facts, next, locals)?;
    set_method_name(ops, method, value, key)?;
    let target = if method.r#static {
        constructor
    } else {
        prototype
    };
    define_method(ops, target, key, value, method.kind);
    Some(())
}

fn reduce_private_method(
    method: &MethodDefinition<'_>,
    name: &oxc::ast::ast::PrivateIdentifier<'_>,
    constructor: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let value = reduce_method(method, ops, facts, next, locals)?;
    ops.push(Op::SetFunctionName {
        function: value,
        name: format!("#{}", name.name),
    });
    let key = facts.private_name(name.span)?;
    ops.push(Op::AppendInstanceField(AppendInstanceFieldOp {
        constructor,
        key: InstanceFieldKeyOp::Private(key),
        initializer: None,
        is_static: method.r#static,
        value: Some(value),
    }));
    Some(())
}

fn reduce_public_field(
    field: &oxc::ast::ast::PropertyDefinition<'_>,
    constructor: u16,
    ops: &mut Vec<Op>,
    static_fields: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let is_static = field.r#static;
    let key = reduce_field_key(field, ops, facts, next, locals)?;
    let initializer = match field.value.as_ref() {
        Some(value) => Some(reduce_field_initializer(value, facts, locals)?),
        None => None,
    };
    let field = AppendInstanceFieldOp {
        constructor,
        key,
        initializer,
        is_static,
        value: None,
    };
    if is_static {
        static_fields.push(Op::AppendInstanceField(field));
    } else {
        ops.push(Op::AppendInstanceField(field));
    }
    Some(())
}

fn reduce_private_field(
    field: &oxc::ast::ast::PropertyDefinition<'_>,
    constructor: u16,
    ops: &mut Vec<Op>,
    static_fields: &mut Vec<Op>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let oxc::ast::ast::PropertyKey::PrivateIdentifier(name) = &field.key else {
        return None;
    };
    let initializer = match field.value.as_ref() {
        Some(value) => Some(reduce_field_initializer(value, facts, locals)?),
        None => None,
    };
    let field = AppendInstanceFieldOp {
        constructor,
        key: InstanceFieldKeyOp::Private(facts.private_name(name.span)?),
        initializer,
        is_static: field.r#static,
        value: None,
    };
    if field.is_static {
        static_fields.push(Op::AppendInstanceField(field));
    } else {
        ops.push(Op::AppendInstanceField(field));
    }
    Some(())
}

fn reduce_field_key(
    field: &oxc::ast::ast::PropertyDefinition<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<InstanceFieldKeyOp> {
    if !field.computed {
        return method_key(&field.key).map(InstanceFieldKeyOp::Static);
    }
    let src =
        crate::reduce::reduce_expression(field.key.as_expression()?, ops, facts, next, locals)?;
    let key = take_register(next);
    ops.push(Op::ToPropertyKey { dst: key, src });
    Some(InstanceFieldKeyOp::Dynamic(key))
}

fn reduce_field_initializer(
    value: &oxc::ast::ast::Expression<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Option<InstanceFieldInitializerOp> {
    let captures = crate::reduce_support::register_base(locals);
    let mut body_locals = locals.clone();
    body_locals.insert("this".to_string(), captures.saturating_add(1));
    let mut next = captures.saturating_add(3);
    let mut body = Vec::new();
    let inherited = (facts.strict, facts.in_function);
    facts.strict = true;
    facts.in_function = true;
    let result = crate::reduce::reduce_expression(value, &mut body, facts, &mut next, &body_locals);
    (facts.strict, facts.in_function) = inherited;
    let result = result?;
    body.push(Op::Return { src: result });
    Some(InstanceFieldInitializerOp { body, captures })
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
        let src = crate::reduce::reduce_expression(
            method.key.as_expression()?,
            ops,
            facts,
            next,
            locals,
        )?;
        let dst = take_register(next);
        ops.push(Op::ToPropertyKey { dst, src });
        return Some(dst);
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
        length: 0,
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
