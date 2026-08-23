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
        ops.push(Op::GetSuperPropertyDynamic {
            dst,
            key,
            base: None,
        });
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
    ops.push(Op::RequireObjectCoercible { src: object });
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
    let value = crate::execute::read_register(registers, src)?.clone();
    if crate::typed_array_ops::is_view(&target) && crate::typed_array_ops::is_index_key(&key) {
        if let Some(result) = crate::typed_array_ops::set_property(&target, &key, &value) {
            crate::execute::write_value(registers, object, result.unwrap_or(target));
            return Ok(());
        }
    }
    if crate::module_bindings::is_namespace(&target) {
        return write_failure(strict);
    }
    if rejects_new_property(&target, &key) {
        return write_failure(strict);
    }
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
    if finish_accessor_set(registers, object, target, key, &value, strict)? {
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
        return finish_primitive_set(target, key, value, strict);
    }
    if let crate::value::Value::Builtin(builtin) = &target {
        if !crate::builtins::object::builtin_property_writable(*builtin, key) {
            return write_failure(strict);
        }
        return set_builtin_property(registers, object, target, key, value);
    }
    if bound_intrinsic_write_blocked(target, key) {
        return write_failure(strict);
    }
    ordinary_set(registers, object, target, key, value, strict)
}

fn bound_intrinsic_write_blocked(target: &crate::value::Value, key: &str) -> bool {
    let crate::value::Value::BoundFunction(bound) = target else {
        return false;
    };
    if !crate::vm::is_intrinsic_bound(bound) {
        return false;
    }
    let crate::value::Value::Builtin(builtin) = bound.target else {
        return false;
    };
    !crate::builtins::object::builtin_property_is_writable(builtin, key)
}

fn finish_accessor_set(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
    strict: bool,
) -> Result<bool, crate::execute::VmError> {
    let Some(setter) = crate::property_define::accessor(target, key, "set") else {
        return Ok(false);
    };
    if matches!(setter, crate::value::Value::Undefined) {
        write_failure(strict)?;
    }
    crate::functions::execute_target(&setter, target, std::slice::from_ref(value))?;
    if let Some(updated) = crate::locals::replacement(target) {
        crate::execute::write_value(registers, object, updated);
    }
    Ok(true)
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
fn finish_primitive_set(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    // Per V8 / ES engine behaviour, assignment to a Symbol primitive
    // value is rejected outright: in strict mode it throws a TypeError;
    // in non-strict mode the write is silently dropped (the auto-boxed
    // wrapper is discarded before any subsequent read).
    if strict && crate::conversion::is_symbol(target) {
        return Err(crate::value::error::throw_type_error(
            "Cannot create property on a symbol value",
        ));
    }
    let receiver = crate::construct::to_object(target)?;
    set_primitive_prototype(&receiver, target, key, value, strict)
}

fn set_primitive_prototype(
    receiver: &crate::value::Value,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    let mut home_proto = primitive_prototype_for(target);
    loop {
        match &home_proto {
            crate::value::Value::Null => break,
            crate::value::Value::Proxy(_) => {
                let trap_result =
                    crate::proxy::proxy_set(&home_proto, key, &value, Some(receiver))?;
                let succeeded = matches!(trap_result, crate::value::Value::Boolean(true));
                if !succeeded && strict {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot assign to read-only property",
                    ));
                }
                return Ok(());
            }
            _ => {}
        }
        let descriptor = crate::builtins::object::descriptor(
            Some(&home_proto),
            Some(&crate::value::Value::String(key.to_string())),
        )?;
        if !matches!(descriptor, crate::value::Value::Undefined) {
            let has_setter = !matches!(
                crate::builtins::object::descriptor(
                    Some(&descriptor),
                    Some(&crate::value::Value::String("set".to_string())),
                ),
                Ok(crate::value::Value::Undefined)
            );
            if has_setter {
                let succeeded = crate::proxy::proxy_set(&home_proto, key, &value, Some(receiver))?;
                let ok = matches!(succeeded, crate::value::Value::Boolean(true));
                if !ok && strict {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot assign to read-only property",
                    ));
                }
                return Ok(());
            }
            let writable = matches!(
                crate::execute::get_property_result(&descriptor, "writable")?,
                crate::value::Value::Boolean(true)
            );
            if !writable {
                if strict {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot assign to read-only property",
                    ));
                }
                return Ok(());
            }
            let own = vec![
                ("value".to_string(), value),
                ("writable".to_string(), crate::value::Value::Boolean(true)),
                ("enumerable".to_string(), crate::value::Value::Boolean(true)),
                (
                    "configurable".to_string(),
                    crate::value::Value::Boolean(true),
                ),
            ];
            let _ = crate::builtins::define_own_property(receiver, key, &own)?;
            return Ok(());
        }
        home_proto = crate::builtins::object::get_prototype_of(Some(&home_proto))?;
    }
    let own = vec![
        ("value".to_string(), value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        ("enumerable".to_string(), crate::value::Value::Boolean(true)),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(true),
        ),
    ];
    let _ = crate::builtins::define_own_property(receiver, key, &own)?;
    Ok(())
}

fn primitive_prototype_for(value: &crate::value::Value) -> crate::value::Value {
    use crate::ops::Builtin;
    use crate::value::Value;
    match value {
        Value::Number(_) => crate::vm::realm_intrinsic(Builtin::NumberPrototype),
        Value::Boolean(_) => crate::vm::realm_intrinsic(Builtin::BooleanPrototype),
        Value::StringUnits(_) => crate::vm::realm_intrinsic(Builtin::StringPrototype),
        Value::BigInt(_) => crate::vm::realm_intrinsic(Builtin::BigIntPrototype),
        Value::String(v) if crate::conversion::is_symbol_string(v) => {
            crate::vm::realm_intrinsic(Builtin::SymbolPrototype)
        }
        Value::String(_) => crate::vm::realm_intrinsic(Builtin::StringPrototype),
        _ => Value::Null,
    }
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
include!("properties_ext.rs");

include!("properties_tail.rs");
