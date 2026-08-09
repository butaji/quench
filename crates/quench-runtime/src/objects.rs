use std::collections::HashMap;

use oxc::ast::ast::{ObjectExpression, ObjectPropertyKind, PropertyKey};

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce(
    object: &ObjectExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let mut properties = Vec::new();
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return None;
        };
        let key = property_key(&property.key)?;
        let value =
            crate::reduce::reduce_expression(&property.value, ops, facts, next_register, locals)?;
        properties.push((key, value));
    }
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeObject {
        dst: register,
        properties,
    });
    Some(register)
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
