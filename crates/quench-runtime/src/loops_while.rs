pub(crate) fn reduce_while(
    statement: &WhileStatement<'_>,
    ops: &mut Vec<Op>, facts: &mut ProgramDb, next_register: &mut u16,
    next_slot: &mut u16, locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let test = reduce_fragment(Some(&statement.test), ops, facts, next_register, locals)?;
    let mut body = Vec::new();
    reduce_body(&statement.body, &mut body, facts, next_register, next_slot, locals)?;
    let [init, test, body, update] = crate::machine::FunctionCode::from_ops_many(
        vec![Vec::new(), test, body, Vec::new()],
    ).try_into().expect("four loop bodies");
    ops.push(Op::Loop { label: None, init, test, body, update, post_test: false });
    Ok(())
}

pub(crate) fn reduce_do_while(
    statement: &DoWhileStatement<'_>,
    ops: &mut Vec<Op>, facts: &mut ProgramDb, next_register: &mut u16,
    next_slot: &mut u16, locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let mut body = Vec::new();
    reduce_body(&statement.body, &mut body, facts, next_register, next_slot, locals)?;
    let test = reduce_fragment(Some(&statement.test), ops, facts, next_register, locals)?;
    let [init, test, body, update] = crate::machine::FunctionCode::from_ops_many(
        vec![Vec::new(), test, body, Vec::new()],
    ).try_into().expect("four loop bodies");
    ops.push(Op::Loop { label: None, init, test, body, update, post_test: true });
    Ok(())
}
