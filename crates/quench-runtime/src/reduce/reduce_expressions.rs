//! Expression reduction helpers.
use crate::{
    arrays,
    facts::ProgramDb,
    identifiers,
    literal::{reduce_literal, reduce_operator},
    ops::Op,
    properties, special, transparent,
};
use oxc::{
    ast::ast::{Argument, Expression, Statement, VariableDeclarationKind},
    syntax::operator::UnaryOperator,
};
use std::collections::HashMap;

const SCRIPT_THIS_SLOT: &str = "\0script_this";
pub fn reduce_if_statement(
    statement: &oxc::ast::ast::IfStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let condition = match &statement.test {
        Expression::BooleanLiteral(condition) => {
            return reduce_static_if(
                statement,
                condition.value,
                ops,
                facts,
                next_register,
                next_slot,
                locals,
            );
        }
        test => reduce_expression(test, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported branch condition".to_string()])?,
    };
    let then_ops = crate::branch::reduce(&statement.consequent, facts, locals)?;
    let else_ops = statement
        .alternate
        .as_ref()
        .map(|alternate| crate::branch::reduce(alternate, facts, locals))
        .transpose()?
        .unwrap_or_default();
    ops.push(Op::Branch {
        condition,
        then_ops,
        else_ops,
    });
    Ok(None)
}
fn reduce_static_if(
    statement: &oxc::ast::ast::IfStatement<'_>,
    condition: bool,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let selected = if condition {
        Some(&statement.consequent)
    } else {
        statement.alternate.as_ref()
    };
    let Some(selected) = selected else {
        return Ok(None);
    };
    match selected {
        Statement::EmptyStatement(_) => Ok(None),
        Statement::BlockStatement(_) => {
            crate::reduce::reduce_statement(selected, ops, facts, next_register, next_slot, locals)
        }
        Statement::ExpressionStatement(expression) => {
            reduce_expression_statement(&expression.expression, ops, facts, next_register, locals)
                .map(Some)
        }
        Statement::VariableDeclaration(declaration) => {
            reduce_declaration(declaration, ops, facts, next_register, next_slot, locals)?;
            Ok(None)
        }
        _ => Err(vec!["Unsupported conditional statement".to_string()]),
    }
}
pub fn reduce_expression_statement(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<u16, Vec<String>> {
    let Some(register) = reduce_expression(expression, ops, facts, next_register, locals) else {
        return Err(vec![format!(
            "Unsupported executable expression: {expression:?}"
        )]);
    };
    Ok(register)
}
pub fn reduce_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    for declarator in &declaration.declarations {
        allocate_pattern_slots(&declarator.id, declaration.kind, next_slot, locals);
        let register = match declarator.init.as_ref() {
            Some(init) => reduce_expression(init, ops, facts, next_register, locals),
            None => Some(crate::reduce_support::emit_undefined(ops, next_register)),
        };
        let Some(register) = register else {
            return Err(vec!["Unsupported variable initializer".to_string()]);
        };
        crate::binding_patterns::bind(&declarator.id, register, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported binding pattern".to_string()])?;
    }
    Ok(())
}

fn allocate_pattern_slots(
    pattern: &oxc::ast::ast::BindingPattern<'_>,
    kind: VariableDeclarationKind,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) {
    for name in crate::binding_patterns::names(pattern) {
        let slot = declaration_slot(kind, &name, next_slot, locals);
        locals.insert(name, slot);
    }
}

fn declaration_slot(
    kind: VariableDeclarationKind,
    name: &str,
    next_slot: &mut u16,
    locals: &HashMap<String, u16>,
) -> u16 {
    if kind == VariableDeclarationKind::Var {
        if let Some(slot) = locals.get(name) {
            return *slot;
        }
    }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    slot
}
pub fn reduce_expression(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Expression::TaggedTemplateExpression(tagged) = expression {
        return super::tagged_template::reduce(tagged, ops, facts, next_register, locals);
    }
    if let Expression::AwaitExpression(await_expression) = expression {
        return reduce_await(await_expression, ops, facts, next_register, locals);
    }
    if let Expression::ParenthesizedExpression(value) = expression {
        return transparent::reduce(value, ops, facts, next_register, locals);
    }
    if let Some(value) = special::reduce(expression, ops, facts, next_register, locals) {
        return Some(value);
    }
    if let Some(register) = reduce_atom(expression, ops, facts, next_register, locals) {
        return Some(register);
    }
    let Expression::BinaryExpression(binary) = expression else {
        return None;
    };
    if binary.operator == oxc::syntax::operator::BinaryOperator::In {
        return reduce_in(binary, ops, facts, next_register, locals);
    }
    let operator = reduce_operator(binary.operator)?;
    let lhs = reduce_expression(&binary.left, ops, facts, next_register, locals)?;
    let rhs = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
    let dst = take_register(next_register);
    ops.push(Op::Binary {
        dst,
        operator,
        lhs,
        rhs,
    });
    Some(dst)
}

fn reduce_await(
    expression: &oxc::ast::ast::AwaitExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let src = reduce_expression(&expression.argument, ops, facts, next_register, locals)?;
    let dst = take_register(next_register);
    ops.push(Op::Await { dst, src });
    Some(dst)
}

fn reduce_in(
    binary: &oxc::ast::ast::BinaryExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let key = reduce_expression(&binary.left, ops, facts, next_register, locals)?;
    let object = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
    let dst = take_register(next_register);
    ops.push(Op::HasPropertyDynamic { dst, object, key });
    Some(dst)
}

fn take_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}

pub fn reduce_unary(
    unary: &oxc::ast::ast::UnaryExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if unary.operator == UnaryOperator::Delete {
        return crate::unary::reduce_delete(&unary.argument, ops, facts, next_register, locals);
    }
    let operator = match unary.operator {
        UnaryOperator::UnaryPlus => crate::ops::UnaryOp::Plus,
        UnaryOperator::UnaryNegation => crate::ops::UnaryOp::Minus,
        UnaryOperator::LogicalNot => crate::ops::UnaryOp::Not,
        UnaryOperator::Void => crate::ops::UnaryOp::Void,
        UnaryOperator::Typeof => crate::ops::UnaryOp::Typeof,
        _ => return None,
    };
    let src = if operator == crate::ops::UnaryOp::Typeof
        && matches!(
            &unary.argument,
            Expression::Identifier(identifier)
                if !locals.contains_key(identifier.name.as_str())
                    && !crate::globals::is_defined(identifier.name.as_str())
        ) {
        crate::reduce_support::emit_undefined(ops, next_register)
    } else {
        reduce_expression(&unary.argument, ops, facts, next_register, locals)?
    };
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Unary { dst, operator, src });
    Some(dst)
}
pub fn reduce_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if is_direct_eval(call, locals) {
        return reduce_direct_eval(call, ops, facts, next_register, locals);
    }
    if let Some(result) = properties::reduce_method_call(call, ops, facts, next_register, locals) {
        return Some(result);
    }
    let callee = reduce_expression(&call.callee, ops, facts, next_register, locals)?;
    let (args, spreads) = reduce_call_arguments(call, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register += 1;
    ops.push(Op::Call {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}

fn is_direct_eval(call: &oxc::ast::ast::CallExpression<'_>, locals: &HashMap<String, u16>) -> bool {
    matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "eval")
        && !locals.contains_key("eval")
}

fn reduce_direct_eval(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let argument = call.arguments.first()?.as_expression()?;
    let source = reduce_expression(argument, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    let mut bindings = locals
        .iter()
        .map(|(name, slot)| (name.clone(), *slot))
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.0.cmp(&right.0));
    ops.push(Op::Eval {
        dst,
        source,
        strict: facts.strict,
        bindings,
        forbidden_var_names: facts.eval_var_barrier.clone(),
    });
    Some(dst)
}
fn reduce_call_arguments(
    call: &oxc::ast::ast::CallExpression<'_>,
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
pub fn reduce_atom(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(expression, Expression::ThisExpression(_)) {
        return Some(reduce_this_atom(ops, next_register, locals));
    }
    if let Some(value) = reduce_literal(expression) {
        return Some(reduce_literal_atom(value, ops, facts, next_register));
    }
    if let Expression::ArrayExpression(array) = expression {
        return arrays::reduce(array, ops, facts, next_register, locals);
    }
    if let Expression::RegExpLiteral(regex) = expression {
        return reduce_regexp_literal(regex, ops, next_register);
    }
    if let Expression::Identifier(identifier) = expression {
        return identifiers::reduce(identifier, ops, facts, next_register, locals);
    }
    None
}

fn reduce_this_atom(
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> u16 {
    if let Some(slot) = locals
        .get("this")
        .or_else(|| locals.get(SCRIPT_THIS_SLOT))
        .or_else(|| locals.get("globalThis"))
        .copied()
    {
        return emit_load_local(ops, next_register, slot);
    }
    crate::reduce_support::emit_undefined(ops, next_register)
}

fn emit_load_local(ops: &mut Vec<Op>, next_register: &mut u16, slot: u16) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal { dst, slot });
    dst
}

fn reduce_literal_atom(
    value: crate::literal::Literal,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    facts.constants.push(crate::facts::ConstantFact {
        value: value.fact.clone(),
    });
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
    ops.push(Op::Construct {
        dst,
        callee,
        args: vec![pattern_register, flags_register],
    });
    Some(dst)
}
