//! Expression reduction helpers.
use crate::{
    arrays,
    facts::ProgramDb,
    identifiers,
    literal::{reduce_literal, reduce_operator},
    ops::Op,
    special, transparent,
};
use oxc::{
    ast::ast::{Expression, ImportPhase, Statement, VariableDeclarationKind},
    syntax::operator::UnaryOperator,
};
use std::collections::HashMap;

const SCRIPT_THIS_SLOT: &str = "\0script_this";
const NEW_TARGET_SLOT: &str = "\0new_target";
#[path = "../calls_reduce.rs"]
pub(crate) mod calls_reduce;
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
    push_branch(ops, condition, then_ops, else_ops)?;
    Ok(None)
}

fn push_branch(
    ops: &mut Vec<Op>,
    condition: u16,
    then_ops: Vec<Op>,
    else_ops: Vec<Op>,
) -> Result<(), Vec<String>> {
    let [then_ops, else_ops] =
        crate::machine::FunctionCode::from_ops_many(vec![then_ops, else_ops])
            .try_into()
            .map_err(|_| vec!["failed to materialize branch bodies".to_string()])?;
    ops.push(Op::Branch {
        condition,
        then_ops,
        else_ops,
    });
    Ok(())
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
    crate::reduce::reduce_statement(selected, ops, facts, next_register, next_slot, locals)
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
        if declaration.kind == VariableDeclarationKind::Const {
            for name in crate::binding_patterns::names(&declarator.id) {
                if let Some(slot) = locals.get(&name).copied() {
                    ops.push(Op::MarkImmutable { slot });
                }
            }
        }
        if declaration.kind == VariableDeclarationKind::Var && declarator.init.is_none() {
            continue;
        }
        let register = match declarator.init.as_ref() {
            Some(init) => reduce_expression(init, ops, facts, next_register, locals),
            None => Some(crate::reduce_support::emit_undefined(ops, next_register)),
        };
        let Some(register) = register else {
            return Err(vec!["Unsupported variable initializer".to_string()]);
        };
        infer_declaration_name(&declarator.id, declarator.init.as_ref(), register, ops);
        crate::binding_patterns::bind(&declarator.id, register, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported binding pattern".to_string()])?;
    }
    Ok(())
}

fn infer_declaration_name(
    pattern: &oxc::ast::ast::BindingPattern<'_>,
    initializer: Option<&Expression<'_>>,
    function: u16,
    ops: &mut Vec<Op>,
) {
    let Some(initializer) = initializer else {
        return;
    };
    if !crate::binding_patterns::anonymous_function_definition(initializer) {
        return;
    }
    let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) = &pattern.kind else {
        return;
    };
    ops.push(Op::SetFunctionName {
        function,
        name: identifier.name.to_string(),
    });
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
    _kind: VariableDeclarationKind,
    name: &str,
    next_slot: &mut u16,
    locals: &HashMap<String, u16>,
) -> u16 {
    if _kind != VariableDeclarationKind::Var {
        let marker = format!("\0lexical-predeclared:{name}");
        if let Some(slot) = locals.get(&marker) {
            return *slot;
        }
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        return slot;
    }
    if let Some(slot) = locals.get(name) {
        return *slot;
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
    match expression {
        Expression::PrivateInExpression(private) => {
            let object = reduce_expression(&private.right, ops, facts, next_register, locals)?;
            let dst = take_register(next_register);
            let name = facts.private_name(private.left.span)?;
            ops.push(Op::HasPrivate { dst, object, name });
            return Some(dst);
        }
        Expression::TaggedTemplateExpression(tagged) => {
            return super::tagged_template::reduce(tagged, ops, facts, next_register, locals);
        }
        Expression::AwaitExpression(await_expression) => {
            return reduce_await(await_expression, ops, facts, next_register, locals);
        }
        Expression::ImportExpression(import) => {
            return reduce_import_expression(import, ops, facts, next_register, locals);
        }
        Expression::ParenthesizedExpression(value) => {
            return transparent::reduce(value, ops, facts, next_register, locals);
        }
        _ => {}
    }
    if let Some(value) = special::reduce(expression, ops, facts, next_register, locals) {
        return Some(value);
    }
    reduce_atom(expression, ops, facts, next_register, locals)
        .or_else(|| reduce_binary(expression, ops, facts, next_register, locals))
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

fn reduce_import_expression(
    import: &oxc::ast::ast::ImportExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let specifier = reduce_expression(&import.source, ops, facts, next_register, locals)?;
    let options = import
        .arguments
        .last()
        .and_then(|argument| reduce_expression(argument, ops, facts, next_register, locals))
        .unwrap_or_else(|| {
            let register = take_register(next_register);
            ops.push(Op::Const {
                dst: register,
                value: crate::ops::Constant::Undefined,
            });
            register
        });
    let phase = take_register(next_register);
    ops.push(Op::Const {
        dst: phase,
        value: crate::ops::Constant::Boolean(matches!(import.phase, Some(ImportPhase::Defer))),
    });
    let capability = take_register(next_register);
    ops.push(Op::MakeBuiltin {
        dst: capability,
        builtin: crate::ops::Builtin::HostCapability(crate::ops::HostCapabilityKind::DynamicImport),
    });
    let imported = take_register(next_register);
    ops.push(Op::CallMethod {
        dst: imported,
        object: capability,
        key: "dynamicImport".to_string(),
        callee: None,
        args: vec![specifier, phase, options],
    });
    let promise = take_register(next_register);
    ops.push(Op::MakeBuiltin {
        dst: promise,
        builtin: crate::ops::Builtin::Promise,
    });
    let dst = take_register(next_register);
    ops.push(Op::CallMethod {
        dst,
        object: promise,
        key: "resolve".to_string(),
        callee: None,
        args: vec![imported],
    });
    Some(dst)
}

fn reduce_in(
    binary: &oxc::ast::ast::BinaryExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Expression::PrivateFieldExpression(private) = &binary.left {
        let object = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
        let dst = take_register(next_register);
        let name = facts.private_name(private.field.span)?;
        ops.push(Op::HasPrivate { dst, object, name });
        return Some(dst);
    }
    let key = reduce_expression(&binary.left, ops, facts, next_register, locals)?;
    let object = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
    let dst = take_register(next_register);
    ops.push(Op::HasPropertyDynamic { dst, object, key });
    Some(dst)
}

fn reduce_binary(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
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
        UnaryOperator::BitwiseNot => crate::ops::UnaryOp::BitwiseNot,
        UnaryOperator::Void => crate::ops::UnaryOp::Void,
        UnaryOperator::Typeof => crate::ops::UnaryOp::Typeof,
        _ => return None,
    };
    let unresolved_typeof = operator == crate::ops::UnaryOp::Typeof
        && crate::unary::is_unresolved_identifier(&unary.argument, locals);
    let src = if unresolved_typeof && dynamic_binding_may_exist(ops) {
        emit_optional_name_lookup(&unary.argument, ops, next_register)?
    } else if unresolved_typeof {
        crate::reduce_support::emit_undefined(ops, next_register)
    } else {
        reduce_expression(&unary.argument, ops, facts, next_register, locals)?
    };
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Unary { dst, operator, src });
    Some(dst)
}

fn emit_optional_name_lookup(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    let mut expression = expression;
    while let Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    let Expression::Identifier(identifier) = expression else {
        return None;
    };
    let dst = take_register(next);
    ops.push(Op::ResolveNameOrUndefined {
        dst,
        name: identifier.name.to_string(),
    });
    Some(dst)
}

fn dynamic_binding_may_exist(ops: &[Op]) -> bool {
    ops.iter().any(op_may_invoke_eval)
}

fn op_may_invoke_eval(op: &Op) -> bool {
    matches!(
        op,
        Op::Eval { .. }
            | Op::Call { .. }
            | Op::OptionalCall { .. }
            | Op::CallMethod { .. }
            | Op::CallSuperMethod { .. }
            | Op::Construct { .. }
            | Op::Await { .. }
            | Op::Yield { .. }
            | Op::Branch { .. }
            | Op::Label { .. }
            | Op::With { .. }
            | Op::Try { .. }
            | Op::Loop { .. }
            | Op::ForIn { .. }
            | Op::ForOf { .. }
            | Op::Switch { .. }
            | Op::Conditional { .. }
    )
}
pub fn reduce_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    calls_reduce::reduce_call(call, ops, facts, next_register, locals)
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
    if let Expression::MetaProperty(property) = expression {
        if property.meta.name == "new" && property.property.name == "target" {
            let slot = *locals.get(NEW_TARGET_SLOT)?;
            return Some(emit_load_local(ops, next_register, slot));
        }
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
    let slot = locals
        .get("this")
        .or_else(|| locals.get(SCRIPT_THIS_SLOT))
        .copied()
        .or_else(|| {
            locals
                .contains_key(SCRIPT_THIS_SLOT)
                .then(|| locals.get("globalThis").copied())
                .flatten()
        });
    if let Some(slot) = slot {
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
    facts.record_fact_in_context(
        value.span,
        crate::facts::ReduceContext::Value,
        crate::facts::Fact::Proven(value.fact.clone()),
    );
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
    let args = vec![pattern_register, flags_register];
    let spreads = vec![false, false];
    ops.push(Op::Construct {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}
