//! OXC-to-residual reduction entry point.
use crate::{
    arrays,
    blocks::reduce as reduce_block,
    control_flow,
    facts::ProgramDb,
    functions, identifiers,
    literal::{reduce_literal, reduce_operator},
    ops::{Constant, Op},
    properties, special,
    statements::reduce_declaration as reduce_declaration_statement,
    transparent,
};
use oxc::{
    allocator::Allocator,
    ast::ast::{AssignmentTarget, BindingPatternKind, Expression, Statement},
    parser::Parser,
    span::SourceType,
    syntax::operator::{AssignmentOperator, UnaryOperator},
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
    predeclare_functions(statements, &mut locals, &mut next_slot);
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

fn predeclare_functions(
    statements: &[Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        let Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        let Some(identifier) = function.id.as_ref() else {
            continue;
        };
        if locals.contains_key(identifier.name.as_str()) {
            continue;
        }
        locals.insert(identifier.name.to_string(), *next_slot);
        *next_slot = next_slot.saturating_add(1);
    }
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
            reduce_block(block, ops, facts, next_register, next_slot, locals)
        }
        Statement::VariableDeclaration(_) | Statement::FunctionDeclaration(_) => {
            reduce_declaration_statement(statement, ops, facts, next_register, next_slot, locals)
        }
        Statement::ReturnStatement(return_statement) => {
            control_flow::reduce_return(return_statement, ops, facts, next_register, locals)
        }
        Statement::ThrowStatement(statement) => {
            control_flow::reduce_throw(statement, ops, facts, next_register, locals)
        }
        Statement::ExpressionStatement(expression) => {
            reduce_expression_statement(&expression.expression, ops, facts, next_register, locals)
                .map(Some)
        }
        statement => crate::statement_control::reduce(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ),
    }
}
pub(crate) fn reduce_function_declaration(
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
    let slot = if let Some(slot) = locals.get(identifier.name.as_str()) {
        *slot
    } else {
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(identifier.name.to_string(), slot);
        slot
    };
    let (parameters, parameter_count, captures) = function_locals(function, locals)?;
    let body_ops = functions::reduce_body(body, facts, parameters, parameter_count, captures)?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunction {
        dst: register,
        body: body_ops,
        params: parameter_count,
        captures,
    });
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(())
}

fn function_locals(
    function: &oxc::ast::ast::Function<'_>,
    locals: &HashMap<String, u16>,
) -> Result<(HashMap<String, u16>, u16, u16), Vec<String>> {
    let (mut parameters, parameter_count) = functions::function_parameters(function)?;
    let captures = locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1));
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    parameters.insert(
        "arguments".to_string(),
        captures.saturating_add(parameter_count),
    );
    parameters.extend(locals.iter().map(|(name, slot)| (name.clone(), *slot)));
    Ok((parameters, parameter_count, captures))
}
pub(crate) fn reduce_if_statement(
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
        .unwrap_or_else(Vec::new);
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

pub(crate) fn finish_program(
    mut ops: Vec<Op>,
    last_value: Option<u16>,
) -> Result<Vec<Op>, Vec<String>> {
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
        return Err(vec![format!(
            "Unsupported executable expression: {expression:?}"
        )]);
    };
    Ok(register)
}
pub(crate) fn reduce_declaration(
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
    if let Some(value) = special::reduce(expression, ops, facts, next_register, locals) {
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
pub(crate) fn reduce_unary(
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

pub(crate) fn reduce_call(
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
pub(crate) fn reduce_assignment(
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
    if matches!(expression, Expression::ThisExpression(_)) {
        return Some(emit_undefined(ops, next_register));
    }
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
