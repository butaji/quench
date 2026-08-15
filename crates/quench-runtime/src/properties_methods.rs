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
    let callee = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetProperty { dst: callee, object, key: key.clone() });
    let (args, spreads) = reduce_call_arguments(call, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::CallMethod {
        dst,
        object,
        key,
        callee: Some(callee),
        args,
        spreads,
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
    let key = super_method_key(&call.callee)?;
    let (args, _) = reduce_call_arguments(call, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    ops.push(Op::CallSuperMethod { dst, key, args });
    Some(dst)
}

fn super_method_key(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::StaticMemberExpression(member) if matches!(member.object, Expression::Super(_)) => {
            Some(member.property.name.to_string())
        }
        Expression::ComputedMemberExpression(member) if matches!(member.object, Expression::Super(_)) => {
            computed_method_key(&member.expression)
        }
        _ => None,
    }
}

fn reduce_call_arguments(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(Vec<u16>, Vec<bool>)> {
    let mut spreads = Vec::new();
    let args = call
        .arguments
        .iter()
        .map(|argument| {
            let (expression, spread) = match argument {
                oxc::ast::ast::Argument::SpreadElement(value) => (&value.argument, true),
                _ => (argument.as_expression()?, false),
            };
            spreads.push(spread);
            crate::reduce::reduce_expression(expression, ops, facts, next, locals)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((args, spreads))
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
    if object.name != "Symbol" {
        return None;
    }
    let name = member.property.name.as_str();
    match name {
        "asyncDispose"
        | "asyncIterator"
        | "dispose"
        | "hasInstance"
        | "isConcatSpreadable"
        | "iterator"
        | "match"
        | "matchAll"
        | "replace"
        | "search"
        | "species"
        | "split"
        | "toPrimitive"
        | "toStringTag"
        | "unscopables" => Some(format!("Symbol.{name}")),
        _ => None,
    }
}
