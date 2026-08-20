use oxc::ast::ast::Statement;

pub(crate) fn reduce_loop_body(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    completion: u16,
) -> Result<Option<u16>, Vec<String>> {
    if let Statement::BlockStatement(block) = statement {
        crate::blocks::hoist_var_names(block, facts.strict, next_slot, locals);
        let mut block_locals = locals.clone();
        crate::reduce_support::predeclare_lexicals(&block.body, &mut block_locals, next_slot);
        return reduce_loop_body_list(
            &block.body,
            ops,
            facts,
            next_register,
            next_slot,
            &mut block_locals,
            completion,
        );
    }
    reduce_loop_body_list(
        std::slice::from_ref(statement),
        ops,
        facts,
        next_register,
        next_slot,
        locals,
        completion,
    )
}

fn reduce_loop_body_list(
    statements: &[Statement<'_>],
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    completion: u16,
) -> Result<Option<u16>, Vec<String>> {
    let mut last = None;
    for statement in statements {
        let pending_abrupt = matches!(
            statement,
            oxc::ast::ast::Statement::BreakStatement(_)
                | oxc::ast::ast::Statement::ContinueStatement(_)
        );
        let start_ops = ops.len();
        if let Some(value) = crate::reduce::reduce_statement(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )? {
            crate::reduce_support::emit_move(ops, completion, value);
            last = Some(value);
        }
        if pending_abrupt {
            crate::blocks::patch_abrupt_value(ops, start_ops, last);
        }
    }
    Ok(last)
}
