use std::collections::HashMap;

use oxc::ast::ast::Expression;

use crate::{facts::ProgramDb, literal::reduce_literal, ops::Op};

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
    let value = crate::execute::get_property(&object_value, &key);
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
    let value = crate::execute::read_register(registers, src)?.clone();
    let result = crate::builtins::set_property(target, &key, value);
    crate::execute::write_value(registers, object, result);
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
    match value {
        crate::value::Value::String(value) => Ok(value.clone()),
        crate::value::Value::Number(value) => Ok(value.to_string()),
        crate::value::Value::Boolean(value) => Ok(value.to_string()),
        crate::value::Value::Null => Ok("null".to_string()),
        crate::value::Value::Undefined => Ok("undefined".to_string()),
        _ => Err(crate::execute::VmError::EvalError(
            "unsupported property key".to_string(),
        )),
    }
}

pub(crate) fn reduce_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let oxc::ast::ast::AssignmentTarget::ComputedMemberExpression(member) = &assignment.left {
        if reduce_literal(&member.expression).is_none() {
            return reduce_dynamic_assignment(
                assignment,
                member,
                ops,
                facts,
                next_register,
                locals,
            );
        }
    }
    let (object_expression, key) = assignment_target(&assignment.left)?;
    if assignment.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let object =
        crate::reduce::reduce_expression(object_expression, ops, facts, next_register, locals)?;
    let value =
        crate::reduce::reduce_expression(&assignment.right, ops, facts, next_register, locals)?;
    ops.push(Op::SetProperty {
        object,
        key,
        src: value,
    });
    store_object_binding(object_expression, object, ops, locals);
    Some(value)
}

fn assignment_target<'a>(
    target: &'a oxc::ast::ast::AssignmentTarget<'a>,
) -> Option<(&'a Expression<'a>, String)> {
    match target {
        oxc::ast::ast::AssignmentTarget::StaticMemberExpression(member) => {
            Some((&member.object, member.property.name.to_string()))
        }
        oxc::ast::ast::AssignmentTarget::ComputedMemberExpression(member) => {
            let key = property_key(&reduce_literal(&member.expression)?.op)?;
            Some((&member.object, key))
        }
        _ => None,
    }
}

fn reduce_dynamic_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if assignment.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    let key =
        crate::reduce::reduce_expression(&member.expression, ops, facts, next_register, locals)?;
    let value =
        crate::reduce::reduce_expression(&assignment.right, ops, facts, next_register, locals)?;
    ops.push(Op::SetPropertyDynamic {
        object,
        key,
        src: value,
    });
    store_object_binding(&member.object, object, ops, locals);
    Some(value)
}

fn store_object_binding(
    expression: &Expression<'_>,
    object: u16,
    ops: &mut Vec<Op>,
    locals: &HashMap<String, u16>,
) {
    let Expression::Identifier(identifier) = expression else {
        return;
    };
    let Some(slot) = locals.get(identifier.name.as_str()) else {
        return;
    };
    ops.push(Op::StoreLocal {
        slot: *slot,
        src: object,
    });
}

pub(crate) fn execute_get(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::GetProperty { dst, object, key } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value =
        crate::execute::get_property(&crate::execute::read_register(registers, *object)?, key);
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
    let (object, key) = match &call.callee {
        Expression::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        Expression::ComputedMemberExpression(member) => {
            let key = property_key(&reduce_literal(&member.expression)?.op)?;
            (&member.object, key)
        }
        _ => return None,
    };
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let mut args = Vec::new();
    for argument in &call.arguments {
        args.push(crate::reduce::reduce_expression(
            argument.as_expression()?,
            ops,
            facts,
            next_register,
            locals,
        )?);
    }
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
