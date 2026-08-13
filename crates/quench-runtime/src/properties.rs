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
    if let Some(setter) = crate::property_define::accessor(&target, &key, "set") {
        if matches!(setter, crate::value::Value::Undefined) {
            return write_failure(strict);
        }
        crate::functions::execute_target(&setter, &target, std::slice::from_ref(&value))?;
        if let Some(updated) = crate::locals::replacement(&target) {
            crate::execute::write_value(registers, object, updated);
        }
        return Ok(());
    }
    if inherited_write_blocked(&target, &key) {
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
        if !crate::builtins::object::builtin_property_writable(*builtin, &key) {
            return write_failure(strict);
        }
        return set_builtin_property(registers, object, &target, &key, value);
    }
    finish_property_write(registers, object, &target, &key, value);
    Ok(())
}

fn inherits_error_prototype(target: &crate::value::Value) -> bool {
    if matches!(
        target,
        crate::value::Value::Builtin(crate::ops::Builtin::ErrorPrototype)
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
    let crate::value::Value::Object(properties) = target else {
        return false;
    };
    properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        && !properties.iter().any(|(name, _)| name == key)
}

pub(crate) fn object_is_extensible(target: &crate::value::Value) -> bool {
    match target {
        crate::value::Value::Object(properties) => {
            !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        }
        crate::value::Value::Array(values) => values.property(NON_EXTENSIBLE).is_none(),
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
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_prevent_extensions(target);
    }
    let crate::value::Value::Object(properties) = target else {
        return Ok(target.clone());
    };
    let mut sealed = properties.as_ref().clone();
    if !sealed.iter().any(|(name, _)| name == NON_EXTENSIBLE) {
        sealed.push((
            NON_EXTENSIBLE.to_string(),
            crate::value::Value::Boolean(true),
        ));
    }
    let result = crate::value::Value::Object(std::rc::Rc::new(sealed));
    crate::locals::replace_value(target, &result);
    if crate::vm::is_global_object(target) {
        let mut registers = Vec::new();
        crate::vm::synchronize_global_object(&mut registers, target, &result);
    }
    Ok(result)
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
    if crate::builtins::descriptor_flag(target, key, "writable") == Some(false) {
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

pub(crate) fn propagate_updated_object(
    registers: &mut Vec<crate::value::Value>,
    argument: Option<u16>,
    old: &crate::value::Value,
    new: &crate::value::Value,
) {
    crate::locals::replace_value(old, new);
    crate::vm::synchronize_global_object(registers, old, new);
    if let Some(argument) = argument {
        crate::execute::write_value(registers, argument, new.clone());
    }
}

pub(crate) fn execute_delete_property(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::DeleteProperty {
        dst,
        object,
        key,
        strict,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?.clone();
    if matches!(
        target,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete a property of null or undefined",
        ));
    }
    let key = dynamic_property_key(&crate::execute::read_register(registers, *key)?)?;
    if matches!(target, crate::value::Value::Proxy(_)) {
        return delete_proxy_property(registers, *dst, &target, &key, *strict);
    }
    let (result, deleted) = crate::builtins::delete_property(target.clone(), &key);
    if !deleted && *strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete non-configurable property",
        ));
    }
    crate::locals::replace_value(&target, &result);
    crate::vm::synchronize_global_object(registers, &target, &result);
    crate::execute::write_value(registers, *object, result);
    crate::execute::write_value(registers, *dst, crate::value::Value::Boolean(deleted));
    Ok(())
}

pub(crate) fn execute_set_prototype(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::SetPrototype { object, prototype } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?.clone();
    let prototype = crate::execute::read_register(registers, *prototype)?.clone();
    if !matches!(
        prototype,
        crate::value::Value::Null | crate::value::Value::Object(_)
    ) {
        return Ok(());
    }
    let updated = crate::builtins::set_property(target.clone(), "\0prototype", prototype);
    crate::locals::replace_value(&target, &updated);
    crate::vm::synchronize_global_object(registers, &target, &updated);
    crate::execute::write_value(registers, *object, updated);
    Ok(())
}

fn property_key(value: &crate::ops::Constant) -> Option<String> {
    match value {
        crate::ops::Constant::String(value) => Some(value.clone()),
        crate::ops::Constant::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

include!("properties_methods.rs");
