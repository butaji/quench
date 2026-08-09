//! OXC-to-residual reduction entry point.
use crate::{
    arrays, blocks, conditional, control_flow,
    facts::ProgramDb,
    functions, identifiers,
    literal::{reduce_literal, reduce_operator},
    logical, objects,
    ops::{Constant, Op},
    properties, sequences, templates, transparent,
};
use oxc::{
    allocator::Allocator,
    ast::ast::{AssignmentTarget, BindingPatternKind, Expression, Statement},
    parser::Parser,
    span::SourceType,
    syntax::operator::{AssignmentOperator, UnaryOperator, UpdateOperator},
};
use std::collections::HashMap;
#[derive(Debug, PartialEq)]
pub struct ResidualProgram {
    pub facts: ProgramDb,
    pub ops: Vec<Op>,
}
pub fn reduce_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::default())
}
pub fn reduce_module_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::mjs())
}
pub fn reduce_source_with_type(
    source: &str,
    source_type: SourceType,
) -> Result<ResidualProgram, Vec<String>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked {
        return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
    }
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|error| format!("SyntaxError: {error}"))
            .collect());
    }
    let (scope_count, symbol_count) = crate::semantic::analyze(&parsed.program)?;
    let mut facts = ProgramDb {
        scope_count,
        symbol_count,
        ..ProgramDb::default()
    };
    let ops = reduce_statements(&parsed.program.body, &mut facts)?;
    Ok(ResidualProgram { facts, ops })
}
fn reduce_statements(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
) -> Result<Vec<Op>, Vec<String>> {
    reduce_statements_with_locals(statements, facts, HashMap::new(), 0)
}
pub(crate) fn reduce_statements_with_locals(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    mut locals: HashMap<String, u16>,
    mut next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops = Vec::new();
    let mut next_register = 0;
    let mut last_value = None;
    for statement in statements {
        if let Some(value) = reduce_statement(
            statement,
            &mut ops,
            facts,
            &mut next_register,
            &mut next_slot,
            &mut locals,
        )? {
            last_value = Some(value);
        }
    }
    finish_program(ops, last_value)
}
pub(crate) fn reduce_statement(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        Statement::EmptyStatement(_) => Ok(None),
        Statement::BlockStatement(block) => {
            blocks::reduce(block, ops, facts, next_register, next_slot, locals)
        }
        Statement::VariableDeclaration(declaration) => {
            reduce_declaration(declaration, ops, facts, next_register, next_slot, locals)?;
            Ok(None)
        }
        Statement::FunctionDeclaration(function) => {
            reduce_function_declaration(function, ops, facts, next_register, next_slot, locals)
                .map(|_| None)
        }
        Statement::ReturnStatement(return_statement) => {
            reduce_return_statement(return_statement, ops, facts, next_register, locals)
        }
        Statement::ThrowStatement(statement) => {
            control_flow::reduce_throw(statement, ops, facts, next_register, locals)
        }
        Statement::IfStatement(statement) => {
            reduce_if_statement(statement, ops, facts, next_register, next_slot, locals)
        }
        Statement::ExpressionStatement(expression) => {
            reduce_expression_statement(&expression.expression, ops, facts, next_register, locals)
                .map(Some)
        }
        _ => Err(vec!["Unsupported executable statement".to_string()]),
    }
}
fn reduce_return_statement(
    statement: &oxc::ast::ast::ReturnStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let register = statement
        .argument
        .as_ref()
        .and_then(|expression| reduce_expression(expression, ops, facts, next_register, locals))
        .or_else(|| Some(emit_undefined(ops, next_register)));
    Ok(register)
}
fn reduce_function_declaration(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let Some(identifier) = function.id.as_ref() else {
        return Err(vec!["Anonymous function declaration".to_string()]);
    };
    let Some(body) = function.body.as_ref() else {
        return Err(vec!["Function without body".to_string()]);
    };
    let (parameters, parameter_count) = functions::function_parameters(function)?;
    let body_ops =
        reduce_statements_with_locals(&body.statements, facts, parameters, parameter_count)?;
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(identifier.name.to_string(), slot);
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunction {
        dst: register,
        body: body_ops,
        params: parameter_count,
    });
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(())
}
fn reduce_if_statement(
    statement: &oxc::ast::ast::IfStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Expression::BooleanLiteral(condition) = &statement.test else {
        return Err(vec!["Dynamic branch is unsupported".to_string()]);
    };
    let selected = if condition.value {
        Some(&statement.consequent)
    } else {
        statement.alternate.as_ref()
    };
    let Some(selected) = selected else {
        return Ok(None);
    };
    match selected {
        Statement::EmptyStatement(_) => Ok(None),
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
fn finish_program(mut ops: Vec<Op>, last_value: Option<u16>) -> Result<Vec<Op>, Vec<String>> {
    if let Some(register) = last_value {
        ops.push(Op::Return { src: register });
    } else {
        ops.push(Op::Const {
            dst: 0,
            value: Constant::Undefined,
        });
        ops.push(Op::Return { src: 0 });
    }
    Ok(ops)
}
fn reduce_expression_statement(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<u16, Vec<String>> {
    let Some(register) = reduce_expression(expression, ops, facts, next_register, locals) else {
        return Err(vec!["Unsupported executable expression".to_string()]);
    };
    Ok(register)
}
fn reduce_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    for declarator in &declaration.declarations {
        let BindingPatternKind::BindingIdentifier(identifier) = &declarator.id.kind else {
            return Err(vec!["Unsupported binding pattern".to_string()]);
        };
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(identifier.name.to_string(), slot);
        let register = match declarator.init.as_ref() {
            Some(init) => reduce_expression(init, ops, facts, next_register, locals),
            None => Some(emit_undefined(ops, next_register)),
        };
        let Some(register) = register else {
            return Err(vec!["Unsupported variable initializer".to_string()]);
        };
        ops.push(Op::StoreLocal {
            slot,
            src: register,
        });
    }
    Ok(())
}
pub(crate) fn emit_undefined(ops: &mut Vec<Op>, next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: register,
        value: Constant::Undefined,
    });
    register
}
pub(crate) fn reduce_expression(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Expression::ParenthesizedExpression(value) = expression {
        return transparent::reduce(value, ops, facts, next_register, locals);
    }
    if let Some(value) = reduce_special(expression, ops, facts, next_register, locals) {
        return Some(value);
    }
    if let Some(register) = reduce_atom(expression, ops, facts, next_register, locals) {
        return Some(register);
    }
    let Expression::BinaryExpression(binary) = expression else {
        return None;
    };
    let operator = reduce_operator(binary.operator)?;
    let lhs = reduce_expression(&binary.left, ops, facts, next_register, locals)?;
    let rhs = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Binary {
        dst,
        operator,
        lhs,
        rhs,
    });
    Some(dst)
}
fn reduce_special(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    match expression {
        Expression::LogicalExpression(value) => {
            logical::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::FunctionExpression(function) => {
            functions::reduce_expression(function, ops, facts, next_register)
        }
        Expression::ObjectExpression(object) => {
            objects::reduce(object, ops, facts, next_register, locals)
        }
        Expression::TemplateLiteral(template) => {
            templates::reduce(template, ops, facts, next_register, locals)
        }
        Expression::SequenceExpression(sequence) => {
            sequences::reduce(sequence, ops, facts, next_register, locals)
        }
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
            properties::reduce(expression, ops, facts, next_register, locals)
        }
        Expression::ConditionalExpression(value) => {
            conditional::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::UnaryExpression(value) => {
            reduce_unary(value, ops, facts, next_register, locals)
        }
        Expression::CallExpression(value) => reduce_call(value, ops, facts, next_register, locals),
        Expression::UpdateExpression(value) => reduce_update(value, ops, next_register, locals),
        Expression::AssignmentExpression(value) => {
            reduce_assignment(value, ops, facts, next_register, locals)
        }
        _ => None,
    }
}
fn reduce_unary(
    unary: &oxc::ast::ast::UnaryExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let operator = match unary.operator {
        UnaryOperator::UnaryPlus => crate::ops::UnaryOp::Plus,
        UnaryOperator::UnaryNegation => crate::ops::UnaryOp::Minus,
        UnaryOperator::LogicalNot => crate::ops::UnaryOp::Not,
        UnaryOperator::Void => crate::ops::UnaryOp::Void,
        UnaryOperator::Typeof => crate::ops::UnaryOp::Typeof,
        _ => return None,
    };
    let src = if operator == crate::ops::UnaryOp::Typeof
        && matches!(&unary.argument, Expression::Identifier(identifier) if !locals.contains_key(identifier.name.as_str()))
    {
        emit_undefined(ops, next_register)
    } else {
        reduce_expression(&unary.argument, ops, facts, next_register, locals)?
    };
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Unary { dst, operator, src });
    Some(dst)
}
fn reduce_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(result) = properties::reduce_method_call(call, ops, facts, next_register, locals) {
        return Some(result);
    }
    let callee = reduce_expression(&call.callee, ops, facts, next_register, locals)?;
    let mut args = Vec::new();
    for argument in &call.arguments {
        let expression = argument.as_expression()?;
        args.push(reduce_expression(
            expression,
            ops,
            facts,
            next_register,
            locals,
        )?);
    }
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Call { dst, callee, args });
    Some(dst)
}
fn reduce_update(
    update: &oxc::ast::ast::UpdateExpression<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) =
        &update.argument
    else {
        return None;
    };
    let slot = *locals.get(identifier.name.as_str())?;
    let old = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal { dst: old, slot });
    let one = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: one,
        value: Constant::Number(1.0),
    });
    let updated = *next_register;
    *next_register = next_register.saturating_add(1);
    let operator = match update.operator {
        UpdateOperator::Increment => crate::ops::BinaryOp::Add,
        UpdateOperator::Decrement => crate::ops::BinaryOp::Subtract,
    };
    ops.push(Op::Binary {
        dst: updated,
        operator,
        lhs: old,
        rhs: one,
    });
    ops.push(Op::StoreLocal { slot, src: updated });
    if update.prefix {
        Some(updated)
    } else {
        Some(old)
    }
}
fn reduce_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let AssignmentTarget::AssignmentTargetIdentifier(identifier) = &assignment.left else {
        return properties::reduce_assignment(assignment, ops, facts, next_register, locals);
    };
    let slot = *locals.get(identifier.name.as_str())?;
    let rhs = reduce_expression(&assignment.right, ops, facts, next_register, locals)?;
    let value = if assignment.operator == AssignmentOperator::Assign {
        rhs
    } else {
        let operator = assignment.operator.to_binary_operator()?;
        let lhs = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::LoadLocal { dst: lhs, slot });
        let dst = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::Binary {
            dst,
            operator: reduce_operator(operator)?,
            lhs,
            rhs,
        });
        dst
    };
    ops.push(Op::StoreLocal { slot, src: value });
    Some(value)
}
fn reduce_atom(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(value) = reduce_literal(expression) {
        let register = *next_register;
        *next_register = next_register.saturating_add(1);
        facts.constants.push(crate::facts::ConstantFact {
            value: value.fact.clone(),
        });
        ops.push(Op::Const {
            dst: register,
            value: value.op,
        });
        return Some(register);
    }
    if let Expression::ArrayExpression(array) = expression {
        return arrays::reduce(array, ops, facts, next_register, locals);
    }
    if let Expression::Identifier(identifier) = expression {
        return identifiers::reduce(identifier, ops, facts, next_register, locals);
    }
    None
}
