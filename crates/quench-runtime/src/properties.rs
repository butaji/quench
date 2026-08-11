use crate::{facts::ProgramDb, literal::reduce_literal, ops::Op};
use oxc::ast::ast::Expression;
use std::collections::HashMap;
const NON_EXTENSIBLE: &str = "\0quench:non_extensible";
pub(crate) fn reduce(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (object, key) = match expression {
        Expression::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        Expression::PrivateFieldExpression(member) => {
            return reduce_private_get(member, ops, facts, next_register, locals);
        }
        Expression::ComputedMemberExpression(member) => {
            let object = crate::reduce::reduce_expression(
                &member.object,
                ops,
                facts,
                next_register,
                locals,
            )?;
            return reduce_dynamic_get(
                &member.expression,
                object,
                ops,
                facts,
                next_register,
                locals,
            );
        }
        _ => return None,
    };
    emit_get(object, key, ops, facts, next_register, locals)
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
    object: &Expression<'_>,
    key: String,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(object, Expression::Super(_)) {
        return Some(emit_super_get(ops, next_register, key));
    }
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetProperty {
        dst: register,
        object,
        key,
    });
    Some(register)
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
    let (object, key, src) = match op {
        Op::SetProperty { object, key, src } => (*object, key.clone(), *src),
        Op::SetPropertyDynamic { object, key, src } => {
            let key = dynamic_property_key(&crate::execute::read_register(registers, *key)?)?;
            (*object, key, *src)
        }
        _ => return Err(crate::execute::VmError::MissingReturn),
    };
    let target = crate::execute::read_register(registers, object)?.clone();
    reject_restricted_property_write(&target, &key)?;
    if rejects_new_property(&target, &key) {
        return strict_write_failure();
    }
    let value = crate::execute::read_register(registers, src)?.clone();
    if crate::vm::is_global_object(&target) && crate::with_scope::set_if_bound(&key, &value)? {
        return Ok(());
    }
    if let Some(setter) = crate::property_define::accessor(&target, &key, "set") {
        if matches!(setter, crate::value::Value::Undefined) {
            return strict_write_failure();
        }
        crate::functions::execute_target(&setter, &target, std::slice::from_ref(&value))?;
        return Ok(());
    }
    if crate::builtins::descriptor_flag(&target, &key, "writable") == Some(false) {
        return strict_write_failure();
    }
    let result = crate::builtins::set_property(target.clone(), &key, value);
    crate::locals::replace_value(&target, &result);
    crate::vm::synchronize_global_object(registers, &target, &result);
    crate::execute::write_value(registers, object, result);
    Ok(())
}
include!("properties_function_name.rs");

fn rejects_new_property(target: &crate::value::Value, key: &str) -> bool {
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
        value => crate::value::is_object(value),
    }
}

pub(crate) fn is_extensible_value(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let target = target.ok_or(crate::execute::VmError::NotCallable)?;
    Ok(crate::value::Value::Boolean(object_is_extensible(target)))
}

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

fn strict_write_failure() -> Result<(), crate::execute::VmError> {
    if crate::super_scope::is_strict() {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    Ok(())
}

include!("properties_copy_data.rs");

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

pub(crate) fn dynamic_property_key(
    value: &crate::value::Value,
) -> Result<String, crate::execute::VmError> {
    crate::conversion::to_property_key(value)
}

pub(crate) fn execute_get(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::GetProperty { dst, object, key } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::get_property_result(
        &crate::execute::read_register(registers, *object)?,
        key,
    )?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn reduce_method_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(result) = reduce_super_method_call(call, ops, facts, next_register, locals) {
        return Some(result);
    }
    let (object, key) = match &call.callee {
        Expression::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        Expression::ComputedMemberExpression(member) => {
            let key = computed_method_key(&member.expression)?;
            (&member.object, key)
        }
        _ => return None,
    };
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let args = reduce_call_arguments(call, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::CallMethod {
        dst,
        object,
        key,
        args,
    });
    Some(dst)
}

fn reduce_super_method_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if !matches!(member.object, Expression::Super(_)) {
        return None;
    }
    let args = reduce_call_arguments(call, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    ops.push(Op::CallSuperMethod {
        dst,
        key: member.property.name.to_string(),
        args,
    });
    Some(dst)
}

fn reduce_call_arguments(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Vec<u16>> {
    call.arguments
        .iter()
        .map(|argument| {
            crate::reduce::reduce_expression(argument.as_expression()?, ops, facts, next, locals)
        })
        .collect()
}

fn computed_method_key(expression: &Expression<'_>) -> Option<String> {
    if let Some(literal) = reduce_literal(expression) {
        return property_key(&literal.op);
    }
    let Expression::StaticMemberExpression(member) = expression else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    (object.name == "Symbol" && member.property.name == "iterator")
        .then(|| "Symbol.iterator".to_string())
}
