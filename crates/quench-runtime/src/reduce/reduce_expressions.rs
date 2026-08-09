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
    ast::ast::{
        Argument, AssignmentOperator, AssignmentTarget, BindingPatternKind, Expression, Statement,
    },
    syntax::operator::UnaryOperator,
};
use std::collections::HashMap;
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
        let BindingPatternKind::BindingIdentifier(identifier) = &declarator.id.kind else {
            return Err(vec!["Unsupported binding pattern".to_string()]);
        };
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(identifier.name.to_string(), slot);
        let register = match declarator.init.as_ref() {
            Some(init) => reduce_expression(init, ops, facts, next_register, locals),
            None => Some(crate::reduce_support::emit_undefined(ops, next_register)),
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
pub fn reduce_expression(
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
            Expression::Identifier(identifier) if !locals.contains_key(identifier.name.as_str())
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
pub fn reduce_assignment(
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
pub fn reduce_atom(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(expression, Expression::ThisExpression(_)) {
        return Some(crate::reduce_support::emit_undefined(ops, next_register));
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
