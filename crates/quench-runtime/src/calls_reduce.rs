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
        return reduce_eval_call(call, ops, facts, next, locals, true);
    }
    if is_indirect_eval(call) {
        return reduce_eval_call(call, ops, facts, next, locals, false);
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
    let receiver = private_call_receiver(&call.callee, ops, facts, next, locals);
    ops.push(Op::Call {
        dst,
        callee,
        receiver,
        args,
        spreads,
    });
    Some(dst)
}

/// When the callee is a private field expression (e.g. `this.#m()`),
/// the call must preserve the enclosing `this` so the private method
/// receives the correct receiver. Emit a load of `this` into a fresh
/// register and return its register index.
fn private_call_receiver(
    callee: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if !matches!(callee, Expression::PrivateFieldExpression(_)) {
        return None;
    }
    let dst = take_register(next);
    if locals.contains_key("\0module_this") {
        ops.push(Op::Const {
            dst,
            value: crate::ops::Constant::Undefined,
        });
    } else if let Some(&slot) = locals.get("this").or_else(|| locals.get("\0script_this")) {
        ops.push(Op::LoadLocal { dst, slot });
    } else {
        ops.push(Op::Const {
            dst,
            value: crate::ops::Constant::Undefined,
        });
    }
    let _ = facts;
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

fn is_direct_eval(call: &CallExpression<'_>, _locals: &HashMap<String, u16>) -> bool {
    matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "eval")
}

fn reduce_eval_call(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
    direct: bool,
) -> Option<u16> {
    let source = reduce_eval_source(call, ops, facts, next, locals)?;
    let callee = load_eval_binding(ops, facts, next, locals);
    let dst = take_register(next);
    let (bindings, reusable_var_names, forbidden_var_names) = if direct {
        let mut bindings = locals
            .iter()
            .map(|(name, slot)| (name.clone(), *slot))
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| left.0.cmp(&right.0));
        (
            bindings,
            reusable_eval_var_names(locals, facts),
            forbidden_eval_var_names(locals, facts),
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    ops.push(Op::Eval {
        dst,
        callee,
        source,
        strict: direct && facts.strict,
        global: !facts.in_function,
        direct,
        tail: false,
        bindings,
        reusable_var_names,
        forbidden_var_names,
    });
    Some(dst)
}

fn load_eval_binding(
    ops: &mut Vec<Op>,
    facts: &ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> u16 {
    let dst = take_register(next);
    if let Some(&slot) = locals.get("eval") {
        ops.push(Op::LoadBinding {
            dst,
            slot,
            name: "eval".to_string(),
            dynamic: facts.in_function && slot < facts.eval_var_scope_start,
        });
    } else {
        ops.push(Op::ResolveName {
            dst,
            key: "eval".to_string(),
        });
    }
    dst
}

fn is_indirect_eval(call: &CallExpression<'_>) -> bool {
    indirect_eval_expression(&call.callee)
}

fn indirect_eval_expression(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ParenthesizedExpression(parenthesized) => {
            matches!(&parenthesized.expression, Expression::SequenceExpression(_))
                && indirect_eval_expression(&parenthesized.expression)
        }
        Expression::SequenceExpression(sequence) if sequence.expressions.len() > 1 => sequence
            .expressions
            .last()
            .is_some_and(|expression| matches!(expression, Expression::Identifier(identifier) if identifier.name == "eval")),
        _ => false,
    }
}

fn forbidden_eval_var_names(locals: &HashMap<String, u16>, facts: &ProgramDb) -> Vec<String> {
    let mut names = facts.eval_var_barrier.clone();
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

fn reusable_eval_var_names(locals: &HashMap<String, u16>, facts: &ProgramDb) -> Vec<String> {
    let mut names = locals
        .iter()
        .filter(|(name, _)| !facts.eval_var_barrier.contains(name))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
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
