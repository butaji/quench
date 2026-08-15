fn reduce_literal_atom(
    value: crate::literal::Literal,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    facts.record_fact_in_context(
        value.span,
        crate::facts::ReduceContext::Value,
        crate::facts::Fact::Proven(value.fact.clone()),
    );
    ops.push(Op::Const {
        dst,
        value: value.op,
    });
    dst
}

fn reduce_regexp_literal(
    regex: &oxc::ast::ast::RegExpLiteral<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) -> Option<u16> {
    let raw = regex.raw.as_ref()?.as_str();
    let separator = raw.rfind('/')?;
    let pattern = &raw[1..separator];
    let flags = &raw[separator + 1..];
    let callee = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeBuiltin {
        dst: callee,
        builtin: crate::ops::Builtin::RegExp,
    });
    let pattern_register = super::tagged_template::emit_string(ops, next_register, pattern);
    let flags_register = super::tagged_template::emit_string(ops, next_register, flags);
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    let args = vec![pattern_register, flags_register];
    let spreads = vec![false, false];
    ops.push(Op::Construct {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}
