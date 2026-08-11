use std::collections::HashMap;

use oxc::ast::ast::{
    CatchClause, ReturnStatement, Statement, ThrowStatement, VariableDeclarationKind,
};

use crate::{facts::ProgramDb, ops::Op};

type HandlerResult = Result<(Option<Vec<Op>>, Option<u16>), Vec<String>>;

pub(crate) fn reduce_return(
    statement: &ReturnStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let register = statement
        .argument
        .as_ref()
        .and_then(|expression| {
            crate::reduce::reduce_expression(expression, ops, facts, next_register, locals)
        })
        .or_else(|| Some(crate::reduce_support::emit_undefined(ops, next_register)));
    if let Some(register) = register {
        if facts.tail_calls && promote_tail_call(ops, register) {
            return Ok(None);
        }
        ops.push(Op::Return { src: register });
    }
    Ok(None)
}

fn promote_tail_call(ops: &mut Vec<Op>, returned: u16) -> bool {
    if promote_conditional_tail(ops, returned) {
        return true;
    }
    let Some(Op::Call {
        dst,
        callee,
        args,
        spreads,
    }) = ops.last()
    else {
        return false;
    };
    if *dst != returned {
        return false;
    }
    let tail_call = Op::TailCall {
        callee: *callee,
        args: args.clone(),
        spreads: spreads.clone(),
    };
    let _ = ops.pop();
    ops.push(tail_call);
    true
}

fn promote_conditional_tail(ops: &mut Vec<Op>, returned: u16) -> bool {
    if !matches!(ops.last(), Some(Op::Conditional { dst, .. }) if *dst == returned) {
        return false;
    }
    let Some(Op::Conditional {
        dst,
        consequent,
        alternate,
        condition,
    }) = ops.pop()
    else {
        return false;
    };
    if !branch_completes(&consequent) || !branch_completes(&alternate) {
        restore_conditional(ops, dst, condition, consequent, alternate);
        return false;
    }
    let mut consequent = consequent;
    let mut alternate = alternate;
    let promoted = promote_branch_tail(&mut consequent) | promote_branch_tail(&mut alternate);
    if promoted {
        ops.push(Op::Branch {
            condition,
            then_ops: consequent,
            else_ops: alternate,
        });
    } else {
        restore_conditional(ops, dst, condition, consequent, alternate);
    }
    promoted
}

fn restore_conditional(
    ops: &mut Vec<Op>,
    dst: u16,
    condition: u16,
    consequent: Vec<Op>,
    alternate: Vec<Op>,
) {
    ops.push(Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    });
}

fn branch_completes(ops: &[Op]) -> bool {
    matches!(ops.last(), Some(Op::Return { .. } | Op::TailCall { .. }))
}

fn promote_branch_tail(ops: &mut Vec<Op>) -> bool {
    let Some(Op::Return { src }) = ops.pop() else {
        return false;
    };
    if promote_tail_call(ops, src) {
        return true;
    }
    ops.push(Op::Return { src });
    false
}

pub(crate) fn reduce_throw(
    statement: &ThrowStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Some(src) =
        crate::reduce::reduce_expression(&statement.argument, ops, facts, next_register, locals)
    else {
        return Err(vec!["Unsupported throw expression".to_string()]);
    };
    ops.push(Op::Throw { src });
    Ok(None)
}

pub(crate) fn reduce_try(
    statement: &oxc::ast::ast::TryStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let (try_locals, next_slot) = hoisted_try_locals(statement, locals);
    let body = reduce_try_body(statement, facts, &try_locals, next_slot)?;
    let (handler, catch_slot) = reduce_try_handler(statement, facts, &try_locals)?;
    let finalizer = reduce_try_finalizer(statement, facts, &try_locals, next_slot)?;
    ops.push(Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
    });
    Ok(())
}

fn reduce_try_body(
    statement: &oxc::ast::ast::TryStatement<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    crate::reduce::reduce_statements_no_tail(
        &statement.block.body,
        facts,
        locals.clone(),
        next_slot,
    )
}

fn reduce_try_handler(
    statement: &oxc::ast::ast::TryStatement<'_>,
    facts: &mut ProgramDb,
    try_locals: &HashMap<String, u16>,
) -> HandlerResult {
    let Some(handler) = statement.handler.as_ref() else {
        return Ok((None, None));
    };
    let (handler_locals, catch_slot, next_slot) = handler_locals(handler, try_locals);
    let mut prefix = catch_binding_prefix(handler, catch_slot, facts, &handler_locals)?;
    let mut ops = crate::reduce::reduce_statements_no_tail(
        &handler.body.body,
        facts,
        handler_locals,
        next_slot,
    )?;
    prefix.append(&mut ops);
    Ok((Some(prefix), catch_slot))
}

fn catch_binding_prefix(
    handler: &CatchClause<'_>,
    catch_slot: Option<u16>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    let Some(parameter) = handler.param.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(catch_slot) = catch_slot else {
        return Ok(Vec::new());
    };
    if matches!(
        parameter.pattern.kind,
        oxc::ast::ast::BindingPatternKind::BindingIdentifier(_)
    ) {
        return Ok(Vec::new());
    }
    let source = crate::reduce_support::register_base(locals);
    let mut next_register = source.saturating_add(1);
    let mut prefix = vec![Op::LoadLocal {
        dst: source,
        slot: catch_slot,
    }];
    crate::binding_patterns::bind(
        &parameter.pattern,
        source,
        &mut prefix,
        facts,
        &mut next_register,
        locals,
    )
    .ok_or_else(|| vec!["Unsupported catch binding pattern".to_string()])?;
    Ok(prefix)
}

fn reduce_try_finalizer(
    statement: &oxc::ast::ast::TryStatement<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    next_slot: u16,
) -> Result<Option<Vec<Op>>, Vec<String>> {
    let Some(finalizer) = statement.finalizer.as_ref() else {
        return Ok(None);
    };
    crate::reduce::reduce_statements_no_tail(&finalizer.body, facts, locals.clone(), next_slot)
        .map(Some)
}

fn handler_locals(
    handler: &CatchClause<'_>,
    locals: &HashMap<String, u16>,
) -> (HashMap<String, u16>, Option<u16>, u16) {
    let mut result = locals.clone();
    let mut next_slot = crate::reduce_support::register_base(&result);
    let Some(parameter) = handler.param.as_ref() else {
        return (result, None, next_slot);
    };
    let slot = next_slot;
    if let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
        &parameter.pattern.kind
    {
        result.insert(identifier.name.to_string(), slot);
        return (result, Some(slot), slot.saturating_add(1));
    }
    next_slot = next_slot.saturating_add(1);
    for name in crate::binding_patterns::names(&parameter.pattern) {
        if let std::collections::hash_map::Entry::Vacant(entry) = result.entry(name) {
            entry.insert(next_slot);
            next_slot = next_slot.saturating_add(1);
        }
    }
    (result, Some(slot), next_slot)
}

pub(crate) fn reduce_try_statement(
    statement: &oxc::ast::ast::TryStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_try(statement, ops, facts, locals).map(|_| None)
}

fn hoisted_try_locals(
    statement: &oxc::ast::ast::TryStatement<'_>,
    locals: &HashMap<String, u16>,
) -> (HashMap<String, u16>, u16) {
    let mut result = locals.clone();
    let mut next_slot = crate::reduce_support::register_base(&result);
    collect_statements_into(&statement.block.body, &mut result, &mut next_slot);
    if let Some(handler) = &statement.handler {
        collect_statements_into(&handler.body.body, &mut result, &mut next_slot);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_statements_into(&finalizer.body, &mut result, &mut next_slot);
    }
    (result, next_slot)
}

fn collect_statements_into(
    statements: &[Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        collect_statement_vars(statement, locals, next_slot);
    }
}

fn collect_statement_vars(
    statement: &Statement<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    match statement {
        Statement::VariableDeclaration(declaration) => {
            collect_var_declaration(declaration, locals, next_slot);
        }
        Statement::BlockStatement(block) => collect_statements_into(&block.body, locals, next_slot),
        Statement::IfStatement(statement) => {
            collect_statement_vars(&statement.consequent, locals, next_slot);
            if let Some(alternate) = &statement.alternate {
                collect_statement_vars(alternate, locals, next_slot);
            }
        }
        Statement::LabeledStatement(_)
        | Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::ForStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_) => collect_nested_body_vars(statement, locals, next_slot),
        Statement::SwitchStatement(statement) => {
            for case in &statement.cases {
                collect_statements_into(&case.consequent, locals, next_slot);
            }
        }
        Statement::TryStatement(statement) => collect_try_parts(statement, locals, next_slot),
        _ => {}
    }
}

fn collect_nested_body_vars(
    statement: &Statement<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    match statement {
        Statement::LabeledStatement(statement) => {
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::WhileStatement(statement) => {
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::DoWhileStatement(statement) => {
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::ForStatement(statement) => {
            if let Some(init) = &statement.init {
                collect_for_init_vars(init, locals, next_slot);
            }
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::ForInStatement(statement) => {
            collect_for_left_vars(&statement.left, locals, next_slot);
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::ForOfStatement(statement) => {
            collect_for_left_vars(&statement.left, locals, next_slot);
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        _ => {}
    }
}

fn collect_try_parts(
    statement: &oxc::ast::ast::TryStatement<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    collect_statements_into(&statement.block.body, locals, next_slot);
    if let Some(handler) = &statement.handler {
        collect_statements_into(&handler.body.body, locals, next_slot);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_statements_into(&finalizer.body, locals, next_slot);
    }
}

fn collect_for_init_vars(
    init: &oxc::ast::ast::ForStatementInit<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    if let oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration) = init {
        collect_var_declaration(declaration, locals, next_slot);
    }
}

fn collect_for_left_vars(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    if let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left {
        collect_var_declaration(declaration, locals, next_slot);
    }
}

fn collect_var_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    if declaration.kind != VariableDeclarationKind::Var {
        return;
    }
    for declarator in &declaration.declarations {
        if let Some(identifier) = declarator.id.get_binding_identifier() {
            insert_hoisted_var(identifier.name.as_str(), locals, next_slot);
        }
    }
}

fn insert_hoisted_var(name: &str, locals: &mut HashMap<String, u16>, next_slot: &mut u16) {
    if locals.contains_key(name) {
        return;
    }
    locals.insert(name.to_string(), *next_slot);
    *next_slot = next_slot.saturating_add(1);
}
