//! OXC AST reduction for call expressions.

use crate::{facts::ProgramDb, ops::Op, reduce::reduce_expression};
use oxc::ast::ast::{Argument, CallExpression};
use std::collections::HashMap;

pub(crate) fn reduce_call(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(result) =
        crate::properties::reduce_method_call(call, ops, facts, next_register, locals)
    {
        return Some(result);
    }
    let callee = reduce_expression(&call.callee, ops, facts, next_register, locals)?;
    let (args, spreads) = process_call_args(call, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Call {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}

fn process_call_args(
    call: &CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(Vec<u16>, Vec<bool>)> {
    let mut args = Vec::new();
    let mut spreads = Vec::new();
    for argument in &call.arguments {
        match argument {
            Argument::SpreadElement(spread) => {
                let src = reduce_expression(&spread.argument, ops, facts, next_register, locals)?;
                args.push(src);
                spreads.push(true);
            }
            _ => {
                let expression = argument.as_expression()?;
                args.push(reduce_expression(
                    expression,
                    ops,
                    facts,
                    next_register,
                    locals,
                )?);
                spreads.push(false);
            }
        }
    }
    Some((args, spreads))
}
