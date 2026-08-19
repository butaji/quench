use std::collections::HashMap;

use oxc::ast::ast::{
    BlockStatement, CatchClause, ReturnStatement, Statement, ThrowStatement,
    VariableDeclarationKind,
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
        if facts.tail_calls {
            mark_eval_tail(ops, register);
            if promote_tail_call(ops, register) {
                return Ok(None);
            }
        }
        ops.push(Op::Return { src: register });
    }
    Ok(None)
}

fn mark_eval_tail(ops: &mut [Op], returned: u16) {
    if let Some(Op::Eval { dst, tail, .. }) = ops.last_mut() {
        if *dst == returned {
            *tail = true;
        }
    }
}

fn promote_tail_call(ops: &mut Vec<Op>, returned: u16) -> bool {
    if promote_conditional_tail(ops, returned) {
        return true;
    }
    let Some(Op::Call {
        dst,
        callee,
        receiver,
        args,
        spreads,
    }) = ops.last()
    else {
        return false;
    };
    if *dst != returned || receiver.is_some() {
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
    let Some(consequent) = consequent.ops().map(<[_]>::to_vec) else {
        return false;
    };
    let Some(alternate) = alternate.ops().map(<[_]>::to_vec) else {
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
        push_promoted_branch(ops, condition, consequent, alternate);
    } else {
        restore_conditional(ops, dst, condition, consequent, alternate);
    }
    promoted
}

fn push_promoted_branch(
    ops: &mut Vec<Op>,
    condition: u16,
    consequent: Vec<Op>,
    alternate: Vec<Op>,
) {
    let mut branches = crate::machine::FunctionCode::from_ops_many(vec![consequent, alternate]);
    let Some(alternate) = branches.pop() else {
        return;
    };
    let Some(consequent) = branches.pop() else {
        return;
    };
    ops.push(Op::Branch {
        condition,
        then_ops: consequent,
        else_ops: alternate,
    });
}

fn restore_conditional(
    ops: &mut Vec<Op>,
    dst: u16,
    condition: u16,
    consequent: Vec<Op>,
    alternate: Vec<Op>,
) {
    let mut branches = crate::machine::FunctionCode::from_ops_many(vec![consequent, alternate]);
    let Some(alternate) = branches.pop() else {
        return;
    };
    let Some(consequent) = branches.pop() else {
        return;
    };
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
    mark_eval_tail(ops, src);
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
    locals: &mut HashMap<String, u16>,
    dst: u16,
    finally_dst: Option<u16>,
) -> Result<(), Vec<String>> {
    let (try_locals, next_slot) = hoisted_try_locals(statement, locals);
    locals.clone_from(&try_locals);
    let body = reduce_try_body(statement, facts, &try_locals, next_slot)?;
    let (handler, catch_slot) = reduce_try_handler(statement, facts, &try_locals)?;
    let finalizer = reduce_try_finalizer(statement, facts, &try_locals, next_slot, finally_dst)?;
    let (body, handler, finalizer) = materialize_try_bodies(body, handler, finalizer)?;
    ops.push(Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        dst,
        finally_dst,
    });
    Ok(())
}

fn materialize_try_bodies(
    body: Vec<Op>,
    handler: Option<Vec<Op>>,
    finalizer: Option<Vec<Op>>,
) -> Result<
    (
        crate::machine::FunctionCode,
        Option<crate::machine::FunctionCode>,
        Option<crate::machine::FunctionCode>,
    ),
    Vec<String>,
> {
    let mut bodies = vec![body];
    let handler_index = handler.map(|body| {
        bodies.push(body);
        bodies.len() - 1
    });
    let finalizer_index = finalizer.map(|body| {
        bodies.push(body);
        bodies.len() - 1
    });
    let mut functions = crate::machine::FunctionCode::from_ops_many(bodies).into_iter();
    let Some(body) = functions.next() else {
        return Err(vec!["Missing try body".to_string()]);
    };
    let handler = handler_index.and_then(|_| functions.next());
    let finalizer = finalizer_index.and_then(|_| functions.next());
    Ok((body, handler, finalizer))
}

fn reduce_try_body(
    statement: &oxc::ast::ast::TryStatement<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    reduce_isolated_block(&statement.block, facts, locals, next_slot)
}

fn reduce_isolated_block(
    block: &BlockStatement<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops = Vec::new();
    let mut next_register = next_slot;
    let mut slot = next_slot;
    let mut block_locals = locals.clone();
    crate::blocks::reduce(
        block,
        &mut ops,
        facts,
        &mut next_register,
        &mut slot,
        &mut block_locals,
    )?;
    Ok(ops)
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
    let mut ops = reduce_isolated_block(&handler.body, facts, &handler_locals, next_slot)?;
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
    finally_dst: Option<u16>,
) -> Result<Option<Vec<Op>>, Vec<String>> {
    let Some(finalizer) = statement.finalizer.as_ref() else {
        return Ok(None);
    };
    let reduce = || reduce_isolated_block(finalizer, facts, locals, next_slot);
    match finally_dst {
        Some(dst) => crate::switch::with_completion(dst, reduce),
        None => crate::switch::suspend_completion(reduce),
    }
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
        preserve_annex_b_outer(&mut result, identifier.name.as_str());
        result.insert(identifier.name.to_string(), slot);
        return (result, Some(slot), slot.saturating_add(1));
    }
    next_slot = next_slot.saturating_add(1);
    for name in crate::binding_patterns::names(&parameter.pattern) {
        preserve_annex_b_outer(&mut result, &name);
        if let std::collections::hash_map::Entry::Vacant(entry) = result.entry(name) {
            entry.insert(next_slot);
            next_slot = next_slot.saturating_add(1);
        }
    }
    (result, Some(slot), next_slot)
}

pub(crate) fn preserve_annex_b_outer(locals: &mut HashMap<String, u16>, name: &str) {
    let key = format!("\0annex-b-outer:{name}");
    if locals.contains_key(&key) {
        return;
    }
    if let Some(slot) = locals.get(name).copied() {
        locals.insert(key, slot);
    }
}

pub(crate) fn reduce_try_statement(
    statement: &oxc::ast::ast::TryStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    next_register: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let dst = crate::switch::take_completion_register(ops, next_register);
    let finally_dst = statement
        .finalizer
        .as_ref()
        .map(|_| crate::switch::take_completion_register(ops, next_register));
    crate::switch::with_completion(dst, || {
        reduce_try(statement, ops, facts, locals, dst, finally_dst)
    })?;
    Ok(Some(dst))
}

include!("control_flow_collect.rs");
