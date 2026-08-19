fn reduce_static_block(
    block: &oxc::ast::ast::StaticBlock<'_>,
    constructor: u16,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Option<Op> {
    let captures = crate::reduce_support::register_base(locals);
    let mut block_locals = locals.clone();
    block_locals.insert("this".to_string(), captures.saturating_add(1));
    block_locals.insert("\0new_target".to_string(), captures.saturating_add(2));
    let mut next_slot = captures.saturating_add(3);
    crate::reduce_support::shadow_function_bindings(
        &block.body,
        &mut block_locals,
        &HashMap::new(),
    );
    crate::reduce_support::predeclare_lexicals(&block.body, &mut block_locals, &mut next_slot);
    block_locals.retain(|name, _| !name.starts_with("\0lexical-predeclared:"));
    let inherited = (facts.strict, facts.in_function);
    facts.strict = true;
    facts.in_function = true;
    let body = crate::reduce::reduce_expression_statements_with_locals(
        &block.body,
        facts,
        block_locals,
        next_slot,
    );
    (facts.strict, facts.in_function) = inherited;
    let body = body.ok()?;
    Some(Op::StaticBlock {
        constructor,
        captures,
        body: crate::machine::FunctionCode::from_ops(body),
    })
}
