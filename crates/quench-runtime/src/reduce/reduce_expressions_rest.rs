fn take_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}

pub fn reduce_unary(
    unary: &oxc::ast::ast::UnaryExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if unary.operator == UnaryOperator::Delete {
        return crate::unary::reduce_delete(&unary.argument, ops, facts, next_register, locals);
    }
    let operator = match unary.operator {
        UnaryOperator::UnaryPlus => crate::ops::UnaryOp::Plus,
        UnaryOperator::UnaryNegation => crate::ops::UnaryOp::Minus,
        UnaryOperator::LogicalNot => crate::ops::UnaryOp::Not,
        UnaryOperator::BitwiseNot => crate::ops::UnaryOp::BitwiseNot,
        UnaryOperator::Void => crate::ops::UnaryOp::Void,
        UnaryOperator::Typeof => crate::ops::UnaryOp::Typeof,
        _ => return None,
    };
    let unresolved_typeof = operator == crate::ops::UnaryOp::Typeof
        && crate::unary::is_unresolved_identifier(&unary.argument, locals);
    // An unresolved identifier may still be bound by the host at runtime
    // (installed globals, module environments), so `typeof x` must emit a
    // real lookup; folding to `undefined` is only sound for known locals.
    let src = if unresolved_typeof {
        emit_optional_name_lookup(&unary.argument, ops, next_register)?
    } else {
        reduce_expression(&unary.argument, ops, facts, next_register, locals)?
    };
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Unary { dst, operator, src });
    Some(dst)
}

fn emit_optional_name_lookup(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    let mut expression = expression;
    while let Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    let Expression::Identifier(identifier) = expression else {
        return None;
    };
    let dst = take_register(next);
    ops.push(Op::ResolveNameOrUndefined {
        dst,
        name: identifier.name.to_string(),
    });
    Some(dst)
}

pub fn reduce_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    calls_reduce::reduce_call(call, ops, facts, next_register, locals)
}
pub fn reduce_atom(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(expression, Expression::ThisExpression(_)) {
        return Some(reduce_this_atom(ops, facts, next_register, locals));
    }
    if let Expression::MetaProperty(property) = expression {
        if property.meta.name == "new" && property.property.name == "target" {
            let slot = *locals.get(NEW_TARGET_SLOT)?;
            return Some(emit_load_local(ops, next_register, slot));
        }
    }
    if let Some(value) = reduce_literal(expression) {
        return Some(reduce_literal_atom(value, ops, facts, next_register));
    }
    if let Expression::ArrayExpression(array) = expression {
        return arrays::reduce(array, ops, facts, next_register, locals);
    }
    if let Expression::RegExpLiteral(regex) = expression {
        return reduce_regexp_literal(regex, ops, next_register);
    }
    if let Expression::Identifier(identifier) = expression {
        return identifiers::reduce(identifier, ops, facts, next_register, locals);
    }
    None
}

fn reduce_this_atom(
    ops: &mut Vec<Op>,
    facts: &ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> u16 {
    if !facts.in_function && locals.contains_key(super::reduce_statements::MODULE_THIS_SLOT) {
        return crate::reduce_support::emit_undefined(ops, next_register);
    }
    if let Some(slot) = locals
        .get("this")
        .or_else(|| locals.get(SCRIPT_THIS_SLOT))
        .or_else(|| locals.get("globalThis"))
        .copied()
    {
        return emit_load_this(ops, next_register, slot);
    }
    crate::reduce_support::emit_undefined(ops, next_register)
}

fn emit_load_this(ops: &mut Vec<Op>, next_register: &mut u16, slot: u16) -> u16 {
    ops.push(Op::CheckInitialized {
        slot,
        name: "this".to_string(),
    });
    emit_load_local(ops, next_register, slot)
}

fn emit_load_local(ops: &mut Vec<Op>, next_register: &mut u16, slot: u16) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal { dst, slot });
    dst
}

include!("reduce_expressions_tail.rs");
