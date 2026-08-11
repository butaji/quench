use std::collections::HashMap;

use oxc::ast::ast::{ObjectExpression, ObjectPropertyKind, PropertyKey, PropertyKind};

use crate::{
    facts::ProgramDb,
    ops::{Op, PropertyDefinitionKind},
};

pub(crate) fn reduce(
    object: &ObjectExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let object_register = take_register(next_register);
    ops.push(Op::MakeObject {
        dst: object_register,
        properties: Vec::new(),
    });
    reduce_properties(object, object_register, ops, facts, next_register, locals)?;
    Some(object_register)
}

fn reduce_properties(
    object: &ObjectExpression<'_>,
    object_register: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    for property in &object.properties {
        reduce_property(property, object_register, ops, facts, next_register, locals)?;
    }
    Some(())
}

fn reduce_property(
    property: &ObjectPropertyKind<'_>,
    object: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    match property {
        ObjectPropertyKind::ObjectProperty(property) => {
            let key = reduce_key(property, ops, facts, next, locals)?;
            let value =
                crate::reduce::reduce_expression(&property.value, ops, facts, next, locals)?;
            if is_proto_initializer(property) {
                ops.push(Op::SetPrototype {
                    object,
                    prototype: value,
                });
                return Some(());
            }
            set_property_name(property, key, value, ops)?;
            define_property(ops, object, key, value, property.kind);
        }
        ObjectPropertyKind::SpreadProperty(spread) => {
            let source =
                crate::reduce::reduce_expression(&spread.argument, ops, facts, next, locals)?;
            ops.push(Op::CopyDataProperties {
                target: object,
                source,
                excluded: Vec::new(),
            });
        }
    }
    Some(())
}

fn define_property(ops: &mut Vec<Op>, object: u16, key: u16, value: u16, kind: PropertyKind) {
    ops.push(Op::DefineProperty {
        object,
        key,
        value,
        kind: definition_kind(kind),
        enumerable: true,
    });
}

fn set_property_name(
    property: &oxc::ast::ast::ObjectProperty<'_>,
    key: u16,
    value: u16,
    ops: &mut Vec<Op>,
) -> Option<()> {
    if !crate::binding_patterns::anonymous_function_definition(&property.value) {
        return Some(());
    }
    let prefix = accessor_prefix(property.kind);
    if property.computed {
        ops.push(Op::SetFunctionNameDynamic {
            function: value,
            key,
            prefix: prefix.map(str::to_string),
        });
        return Some(());
    }
    let mut name = property_key(&property.key)?;
    if let Some(prefix) = prefix {
        name = format!("{prefix} {name}");
    }
    ops.push(Op::SetFunctionName {
        function: value,
        name,
    });
    Some(())
}

fn accessor_prefix(kind: PropertyKind) -> Option<&'static str> {
    match kind {
        PropertyKind::Get => Some("get"),
        PropertyKind::Set => Some("set"),
        PropertyKind::Init => None,
    }
}

fn is_proto_initializer(property: &oxc::ast::ast::ObjectProperty<'_>) -> bool {
    !property.computed
        && property.kind == PropertyKind::Init
        && property_key(&property.key).as_deref() == Some("__proto__")
}

fn reduce_key(
    property: &oxc::ast::ast::ObjectProperty<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if property.computed {
        let src = crate::reduce::reduce_expression(
            property.key.as_expression()?,
            ops,
            facts,
            next,
            locals,
        )?;
        let dst = take_register(next);
        ops.push(Op::ToPropertyKey { dst, src });
        return Some(dst);
    }
    let key = property_key(&property.key)?;
    let register = take_register(next);
    ops.push(Op::Const {
        dst: register,
        value: crate::ops::Constant::String(key),
    });
    Some(register)
}

fn definition_kind(kind: PropertyKind) -> PropertyDefinitionKind {
    match kind {
        PropertyKind::Init => PropertyDefinitionKind::Data,
        PropertyKind::Get => PropertyDefinitionKind::Get,
        PropertyKind::Set => PropertyDefinitionKind::Set,
    }
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}

fn property_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
        PropertyKey::PrivateIdentifier(_) => None,
        PropertyKey::StringLiteral(value) => Some(value.value.to_string()),
        PropertyKey::NumericLiteral(value) => Some(value.value.to_string()),
        _ => None,
    }
}
