pub(crate) fn reduce_while(
    statement: &WhileStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_counted_loop(
        ops,
        facts,
        next_register,
        next_slot,
        locals,
        Some(&statement.test),
        &statement.body,
        false,
    )
}

pub(crate) fn reduce_do_while(
    statement: &DoWhileStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_counted_loop(
        ops,
        facts,
        next_register,
        next_slot,
        locals,
        Some(&statement.test),
        &statement.body,
        true,
    )
}

fn reduce_counted_loop(
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    test: Option<&oxc::ast::ast::Expression<'_>>,
    body: &oxc::ast::ast::Statement<'_>,
    post_test: bool,
) -> Result<Option<u16>, Vec<String>> {
    let dst = crate::reduce_support::emit_undefined(ops, next_register);
    let test = reduce_fragment(test, ops, facts, next_register, locals)?;
    let mut body_ops = Vec::new();
    let _last = reduce_loop_body(
        body,
        &mut body_ops,
        facts,
        next_register,
        next_slot,
        locals,
        dst,
    )?;
    let [init, test, body, update] = crate::machine::FunctionCode::from_ops_many(vec![
        Vec::new(),
        test,
        body_ops,
        Vec::new(),
    ])
    .try_into()
    .expect("four loop bodies");
    ops.push(Op::Loop {
        label: None,
        init,
        test,
        body,
        update,
        post_test,
        dst,
        per_iteration: Vec::new(),
    });
    Ok(Some(dst))
}
