use crate::{facts::ProgramDb, literal::reduce_literal, ops::Op};
use oxc::ast::ast::Expression;
use std::collections::HashMap;
const NON_EXTENSIBLE: &str = "\0quench:non_extensible";
include!("properties_optional.rs");
pub(crate) fn reduce(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (object, key, optional) = match expression {
        Expression::StaticMemberExpression(member) => (
            &member.object,
            member.property.name.to_string(),
            member.optional,
        ),
        Expression::PrivateFieldExpression(member) => {
            return reduce_private_get(member, ops, facts, next_register, locals);
        }
        Expression::ComputedMemberExpression(member) => {
            return reduce_computed_get(member, ops, facts, next_register, locals);
        }
        _ => return None,
    };
    emit_get(object, key, optional, ops, facts, next_register, locals)
}

fn reduce_computed_get(
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(member.object, Expression::Super(_)) {
        ops.push(Op::CheckSuperThis);
        let key = crate::reduce::reduce_expression(
            &member.expression,
            ops,
            facts,
            next_register,
            locals,
        )?;
        let dst = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::GetSuperPropertyDynamic { dst, key });
        return Some(dst);
    }
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    reduce_dynamic_get(
        &member.expression,
        object,
        ops,
        facts,
        next_register,
        locals,
    )
}
fn reduce_private_get(
    member: &oxc::ast::ast::PrivateFieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let name = facts.private_name(member.field.span)?;
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetPrivate { dst, object, name });
    Some(dst)
}
fn emit_get(
    object_expression: &Expression<'_>,
    key: String,
    optional: bool,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(object_expression, Expression::Super(_)) {
        return Some(emit_super_get(ops, next_register, key));
    }
    let object =
        crate::reduce::reduce_expression(object_expression, ops, facts, next_register, locals)?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    let op = if optional || is_optional_chain_value(object_expression) {
        Op::OptionalGet {
            dst: register,
            object,
            key,
        }
    } else {
        Op::GetProperty {
            dst: register,
            object,
            key,
        }
    };
    ops.push(op);
    Some(register)
}

fn is_optional_chain_value(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ChainExpression(_) => true,
        Expression::ParenthesizedExpression(parenthesized) => {
            is_optional_chain_value(&parenthesized.expression)
        }
        Expression::StaticMemberExpression(member) => {
            member.optional || is_optional_chain_value(&member.object)
        }
        Expression::ComputedMemberExpression(member) => {
            member.optional || is_optional_chain_value(&member.object)
        }
        _ => false,
    }
}
fn emit_super_get(ops: &mut Vec<Op>, next_register: &mut u16, key: String) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetSuperProperty { dst, key });
    dst
}
fn reduce_dynamic_get(
    key_expression: &Expression<'_>,
    object: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let key = crate::reduce::reduce_expression(key_expression, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetPropertyDynamic { dst, object, key });
    Some(dst)
}
pub(crate) fn execute_get_dynamic(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::GetPropertyDynamic { dst, object, key } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let object_value = crate::execute::read_register(registers, *object)?;
    let key_value = crate::execute::read_register(registers, *key)?;
    let key = dynamic_property_key(&key_value)?;
    let value = crate::execute::get_property_result(&object_value, &key)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
pub(crate) fn execute_set_property(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let (object, key, src, strict) = set_property_parts(registers, op)?;
    let target = crate::execute::read_register(registers, object)?.clone();
    reject_nullish_property_write(&target)?;
    reject_restricted_property_write(&target, &key)?;
    if rejects_new_property(&target, &key) {
        return write_failure(strict);
    }
    let value = crate::execute::read_register(registers, src)?.clone();
    if matches!(target, crate::value::Value::Proxy(_)) {
        return assign_proxy_set(registers, object, &target, &key, value);
    }
    if crate::vm::is_global_object(&target) && crate::with_scope::set_if_bound(&key, &value)? {
        return Ok(());
    }
    if key == "stack" && inherits_error_prototype(&target) {
        crate::vm::execute_builtin_with_receiver(
            crate::ops::Builtin::ErrorPrototypeStackSetter,
            &[value],
            Some(&target),
        )?;
        return Ok(());
    }
    finish_set_property(registers, object, &target, &key, value, strict)
}

fn finish_set_property(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    if let Some(setter) = crate::property_define::accessor(target, key, "set") {
        if matches!(setter, crate::value::Value::Undefined) {
            return write_failure(strict);
        }
        crate::functions::execute_target(&setter, target, std::slice::from_ref(&value))?;
        if let Some(updated) = crate::locals::replacement(target) {
            crate::execute::write_value(registers, object, updated);
        }
        return Ok(());
    }
    if inherited_write_blocked(target, key) {
        return write_failure(strict);
    }
    if matches!(
        target,
        crate::value::Value::String(_)
            | crate::value::Value::StringUnits(_)
            | crate::value::Value::Number(_)
            | crate::value::Value::Boolean(_)
            | crate::value::Value::BigInt(_)
    ) {
        return write_failure(strict);
    }
    if let crate::value::Value::Builtin(builtin) = &target {
        if !crate::builtins::object::builtin_property_writable(*builtin, key) {
            return write_failure(strict);
        }
        return set_builtin_property(registers, object, target, key, value);
    }
    finish_property_write(registers, object, target, key, value);
    Ok(())
}

fn inherits_error_prototype(target: &crate::value::Value) -> bool {
    if matches!(
        target,
        crate::value::Value::Builtin(
            crate::ops::Builtin::ErrorPrototype | crate::ops::Builtin::AggregateErrorPrototype,
        )
    ) {
        return true;
    }
    match target {
        crate::value::Value::Object(properties) => properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0prototype").then_some(value))
            .is_some_and(inherits_error_prototype),
        crate::value::Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .and_then(|properties| {
                properties
                    .iter()
                    .rev()
                    .find_map(|(name, value)| (name == "\0prototype").then_some(value.clone()))
            })
            .is_some_and(|prototype| inherits_error_prototype(&prototype)),
        _ => false,
    }
}
fn set_property_parts(
    registers: &[crate::value::Value],
    op: &Op,
) -> Result<(u16, String, u16, bool), crate::execute::VmError> {
    match op {
        Op::SetProperty {
            object,
            key,
            src,
            strict,
        } => Ok((*object, key.clone(), *src, *strict)),
        Op::SetPropertyDynamic {
            object,
            key,
            src,
            strict,
        } => Ok((
            *object,
            dynamic_property_key(&crate::execute::read_register(registers, *key)?)?,
            *src,
            *strict,
        )),
        _ => Err(crate::execute::VmError::MissingReturn),
    }
}
fn finish_property_write(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) {
    let result = crate::builtins::set_property(target.clone(), key, value);
    crate::locals::replace_value(target, &result);
    crate::vm::synchronize_global_object(registers, target, &result);
    crate::execute::write_value(registers, object, result);
}
fn reject_nullish_property_write(
    target: &crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    if matches!(
        target,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign a property of null or undefined",
        ));
    }
    Ok(())
}
include!("properties_function_name.rs");

fn set_builtin_property(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    let properties = std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        ("enumerable".to_string(), crate::value::Value::Boolean(true)),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(true),
        ),
    ]));
    let updated = crate::builtins::define_own_property(target, key, properties.as_ref())?;
    crate::execute::write_value(registers, object, updated);
    Ok(())
}

pub(crate) fn rejects_new_property(target: &crate::value::Value, key: &str) -> bool {
    match target {
        crate::value::Value::Object(properties) => marked_without_key(properties, key),
        crate::value::Value::Function(function) => {
            let properties = function.properties.borrow();
            marked_without_key(&properties, key)
        }
        crate::value::Value::BoundFunction(bound) => {
            let properties = bound.properties.borrow();
            marked_without_key(&properties, key)
        }
        crate::value::Value::Array(values) => {
            let own = key == "length"
                || crate::arrays::array_index(key)
                    .is_some_and(|index| values.has_index(index as usize))
                || values.property(key).is_some();
            values.property(NON_EXTENSIBLE).is_some() && !own
        }
        _ => false,
    }
}

fn marked_without_key(properties: &[(String, crate::value::Value)], key: &str) -> bool {
    properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        && !properties.iter().any(|(name, _)| name == key)
}

pub(crate) fn object_is_extensible(target: &crate::value::Value) -> bool {
    if let crate::value::Value::BindingCell(cell) = target {
        return object_is_extensible(&cell.borrow());
    }
    match target {
        crate::value::Value::Builtin(crate::ops::Builtin::ThrowTypeError) => false,
        crate::value::Value::Object(properties) => {
            !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        }
        crate::value::Value::Array(values) => values.property(NON_EXTENSIBLE).is_none(),
        crate::value::Value::Function(function) => !function
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == NON_EXTENSIBLE),
        crate::value::Value::BoundFunction(bound) => !bound
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == NON_EXTENSIBLE),
        value => crate::value::is_object(value),
    }
}

pub(crate) fn is_extensible_value(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let target = target.ok_or(crate::execute::VmError::NotCallable)?;
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_is_extensible(target);
    }
    Ok(crate::value::Value::Boolean(object_is_extensible(target)))
}

include!("properties_integrity.rs");

pub(crate) fn prevent_extensions(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let Some(target) = target else {
        return Err(crate::value::error::throw_type_error("Object expected"));
    };
    if let crate::value::Value::BindingCell(cell) = target {
        let current = cell.borrow().clone();
        let updated = prevent_extensions(Some(&current))?;
        *cell.borrow_mut() = updated;
        return Ok(target.clone());
    }
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_prevent_extensions(target);
    }
    let result = mark_non_extensible(target);
    crate::locals::replace_value(target, &result);
    if crate::vm::is_global_object(target) {
        let mut registers = Vec::new();
        crate::vm::synchronize_global_object(&mut registers, target, &result);
    }
    Ok(result)
}

fn mark_non_extensible(target: &crate::value::Value) -> crate::value::Value {
    match target {
        crate::value::Value::Object(properties) => {
            let mut sealed = properties.as_ref().clone();
            push_non_extensible(&mut sealed);
            crate::value::Value::Object(std::rc::Rc::new(sealed))
        }
        crate::value::Value::Array(values) => {
            let mut values = std::rc::Rc::clone(values);
            std::rc::Rc::make_mut(&mut values)
                .set_property(NON_EXTENSIBLE, crate::value::Value::Boolean(true));
            crate::value::Value::Array(values)
        }
        crate::value::Value::Function(function) => {
            mark_properties(&mut function.properties.borrow_mut());
            target.clone()
        }
        crate::value::Value::BoundFunction(bound) => {
            mark_properties(&mut bound.properties.borrow_mut());
            target.clone()
        }
        _ => target.clone(),
    }
}

fn mark_properties(properties: &mut Vec<(String, crate::value::Value)>) {
    if !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE) {
        properties.push((
            NON_EXTENSIBLE.to_string(),
            crate::value::Value::Boolean(true),
        ));
    }
}

fn reject_restricted_property_write(
    target: &crate::value::Value,
    key: &str,
) -> Result<(), crate::execute::VmError> {
    if matches!(&target, crate::value::Value::Array(values) if values.is_strict_arguments() && key == "callee")
    {
        return Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        ));
    }
    if crate::vm::has_restricted_function_property(target, key) {
        return Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        ));
    }
    Ok(())
}

fn inherited_write_blocked(target: &crate::value::Value, key: &str) -> bool {
    // Prototype objects do not truly own `length`/`name`; assigning them
    // creates an own property that shadows the callable metadata.
    let prototype_meta_key = matches!(key, "length" | "name")
        && matches!(target, crate::value::Value::Builtin(builtin) if crate::builtin_meta::is_prototype(*builtin));
    if !prototype_meta_key
        && crate::builtins::descriptor_flag(target, key, "writable") == Some(false)
    {
        return true;
    }
    matches!(
        crate::property_define::accessor(target, key, "writable"),
        Some(crate::value::Value::Boolean(false))
    )
}
fn write_failure(strict: bool) -> Result<(), crate::execute::VmError> {
    if strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    Ok(())
}

include!("properties_assign.rs");
include!("properties_copy_data.rs");
include!("properties_reflect_set.rs");

include!("properties_delete.rs");
include!("properties_methods.rs");
include!("properties_prototype.rs");
