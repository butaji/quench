pub(crate) fn reduce_yield(
    expression: &oxc::ast::ast::YieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if expression.delegate {
        return reduce_yield_star(expression, ops, facts, next, locals);
    }
    let src = match expression.argument.as_ref() {
        Some(argument) => crate::reduce::reduce_expression(argument, ops, facts, next, locals)?,
        None => crate::reduce_support::emit_undefined(ops, next),
    };
    ops.push(Op::Yield { src });
    Some(src)
}

fn reduce_yield_star(
    expression: &oxc::ast::ast::YieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let source =
        crate::reduce::reduce_expression(expression.argument.as_ref()?, ops, facts, next, locals)?;
    let dst = *next;
    let iterator = next.saturating_add(1);
    *next = next.saturating_add(2);
    ops.push(Op::YieldStar { dst, source, iterator });
    Some(dst)
}
