//! OXC AST reduction for call expressions.

use crate::{
    facts::ProgramDb,
    ops::{ArrayElement, Op},
    reduce::reduce_expression,
};
use oxc::ast::ast::{Argument, CallExpression, Expression};
use std::collections::HashMap;

pub(crate) fn reduce_call(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if call.optional || has_optional_chain_callee(&call.callee) {
        return crate::special::reduce_optional_call(call, ops, facts, next, locals);
    }
    if is_direct_eval(call, locals) {
        return reduce_direct_eval(call, ops, facts, next, locals);
    }
    if matches!(call.callee, Expression::Super(_)) {
        return reduce_super_constructor_call(call, ops, facts, next, locals);
    }
    if let Some(result) = crate::properties::reduce_method_call(call, ops, facts, next, locals) {
        return Some(result);
    }
    let callee = reduce_expression(&call.callee, ops, facts, next, locals)?;
    let (args, spreads) = reduce_call_arguments(call, ops, facts, next, locals)?;
    let dst = take_register(next);
    ops.push(Op::Call {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}

fn has_optional_chain_callee(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ChainExpression(_) => true,
        Expression::ParenthesizedExpression(parenthesized) => {
            has_optional_chain_callee(&parenthesized.expression)
        }
        Expression::StaticMemberExpression(member) => {
            member.optional || has_optional_chain_callee(&member.object)
        }
        Expression::ComputedMemberExpression(member) => {
            member.optional || has_optional_chain_callee(&member.object)
        }
        _ => false,
    }
}

fn reduce_super_constructor_call(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (args, spreads) = reduce_call_arguments(call, ops, facts, next, locals)?;
    let dst = take_register(next);
    ops.push(Op::CallSuperConstructor { dst, args, spreads });
    Some(dst)
}

pub(crate) fn reduce_call_arguments(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(Vec<u16>, Vec<bool>)> {
    reduce_arguments(&call.arguments, ops, facts, next, locals)
}

pub(crate) fn reduce_arguments(
    arguments: &[Argument<'_>],
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(Vec<u16>, Vec<bool>)> {
    let mut args = Vec::new();
    let mut spreads = Vec::new();
    for argument in arguments {
        let (expression, spread) = match argument {
            Argument::SpreadElement(spread) => (&spread.argument, true),
            _ => (argument.as_expression()?, false),
        };
        args.push(reduce_expression(expression, ops, facts, next, locals)?);
        spreads.push(spread);
    }
    Some((args, spreads))
}

fn is_direct_eval(call: &CallExpression<'_>, locals: &HashMap<String, u16>) -> bool {
    matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "eval")
        && !locals.contains_key("eval")
}

fn reduce_direct_eval(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let source = reduce_eval_source(call, ops, facts, next, locals)?;
    let dst = take_register(next);
    let mut bindings = locals
        .iter()
        .map(|(name, slot)| (name.clone(), *slot))
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.0.cmp(&right.0));
    let reusable_var_names = reusable_eval_var_names(locals, facts);
    let forbidden_var_names = forbidden_eval_var_names(locals, facts);
    ops.push(Op::Eval {
        dst,
        source,
        strict: facts.strict,
        global: !facts.in_function,
        direct: true,
        bindings,
        reusable_var_names,
        forbidden_var_names,
    });
    Some(dst)
}

fn forbidden_eval_var_names(locals: &HashMap<String, u16>, facts: &ProgramDb) -> Vec<String> {
    let mut names = facts.eval_var_barrier.clone();
    if facts.eval_arrow_scope
        && locals
            .get("arguments")
            .is_some_and(|slot| *slot < facts.eval_var_scope_start)
    {
        names.push("arguments".to_string());
    }
    names.sort_unstable();
    names.dedup();
    names
}

fn reusable_eval_var_names(locals: &HashMap<String, u16>, facts: &ProgramDb) -> Vec<String> {
    let mut names = locals
        .iter()
        .filter(|(name, slot)| {
            **slot >= facts.eval_var_scope_start && !facts.eval_var_barrier.contains(name)
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    if facts.eval_arrow_scope
        && locals
            .get("arguments")
            .is_some_and(|slot| *slot >= facts.eval_var_scope_start)
    {
        names.push("arguments".to_string());
    }
    names.sort_unstable();
    names.dedup();
    names
}

fn reduce_eval_source(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if call.arguments.is_empty() {
        return Some(crate::reduce_support::emit_undefined(ops, next));
    }
    let (args, spreads) = reduce_call_arguments(call, ops, facts, next, locals)?;
    if spreads.iter().all(|spread| !spread) {
        return args.first().copied();
    }
    Some(materialize_first_argument(args, spreads, ops, next))
}

fn materialize_first_argument(
    args: Vec<u16>,
    spreads: Vec<bool>,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> u16 {
    let elements = args
        .into_iter()
        .zip(spreads)
        .map(|(src, spread)| match spread {
            true => ArrayElement::Spread(src),
            false => ArrayElement::Value(src),
        })
        .collect();
    let list = take_register(next);
    ops.push(Op::BuildArray {
        dst: list,
        elements,
    });
    let source = take_register(next);
    ops.push(Op::GetProperty {
        dst: source,
        object: list,
        key: "0".to_string(),
    });
    source
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}
