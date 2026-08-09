use std::collections::HashMap;

use oxc::{
    ast::ast::{Expression, ForStatement, ForStatementInit, Statement},
    syntax::operator::BinaryOperator,
};

use crate::{
    facts::ProgramDb,
    literal::reduce_literal,
    ops::{Constant, Op},
};

pub(crate) fn reduce_for(
    statement: &ForStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let (name, start) = numeric_init(statement.init.as_ref())?;
    let (limit, step) = numeric_test_and_update(statement, &name)?;
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(name, slot);
    let mut current = start;
    let mut count = 0;
    let mut context = LoopContext {
        ops,
        facts,
        next_register,
        next_slot,
        locals,
    };
    while (step > 0.0 && current < limit) || (step < 0.0 && current > limit) {
        if count >= 1_000 {
            return Err(vec!["Static loop exceeds reduction bound".to_string()]);
        }
        emit_iteration(statement, current, slot, &mut context)?;
        current += step;
        count += 1;
    }
    Ok(())
}

struct LoopContext<'a> {
    ops: &'a mut Vec<Op>,
    facts: &'a mut ProgramDb,
    next_register: &'a mut u16,
    next_slot: &'a mut u16,
    locals: &'a mut HashMap<String, u16>,
}

fn emit_iteration(
    statement: &ForStatement<'_>,
    current: f64,
    slot: u16,
    context: &mut LoopContext<'_>,
) -> Result<(), Vec<String>> {
    let register = *context.next_register;
    *context.next_register = context.next_register.saturating_add(1);
    context.ops.push(Op::Const {
        dst: register,
        value: Constant::Number(current),
    });
    context.ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    reduce_body(
        &statement.body,
        context.ops,
        context.facts,
        context.next_register,
        context.next_slot,
        context.locals,
    )
}

fn numeric_init(init: Option<&ForStatementInit<'_>>) -> Result<(String, f64), Vec<String>> {
    let Some(ForStatementInit::VariableDeclaration(declaration)) = init else {
        return Err(vec!["Dynamic for initializer is unsupported".to_string()]);
    };
    let Some(declarator) = declaration.declarations.first() else {
        return Err(vec!["Empty for initializer".to_string()]);
    };
    let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) = &declarator.id.kind
    else {
        return Err(vec!["Unsupported for binding".to_string()]);
    };
    let Some(Expression::NumericLiteral(number)) = declarator.init.as_ref() else {
        return Err(vec!["Dynamic for initializer is unsupported".to_string()]);
    };
    Ok((identifier.name.to_string(), number.value))
}

fn numeric_test_and_update(
    statement: &ForStatement<'_>,
    name: &str,
) -> Result<(f64, f64), Vec<String>> {
    let Some(Expression::BinaryExpression(binary)) = statement.test.as_ref() else {
        return Err(vec!["Dynamic for test is unsupported".to_string()]);
    };
    let Expression::Identifier(identifier) = &binary.left else {
        return Err(vec!["Unsupported for test target".to_string()]);
    };
    if identifier.name != name {
        return Err(vec!["Mismatched for test target".to_string()]);
    }
    let Some(number) = reduce_literal(&binary.right).and_then(|literal| match literal.op {
        Constant::Number(value) => Some(value),
        _ => None,
    }) else {
        return Err(vec!["Dynamic for limit is unsupported".to_string()]);
    };
    let step = update_step(statement)?;
    let valid = matches!(
        (binary.operator, step > 0.0),
        (
            BinaryOperator::LessThan | BinaryOperator::LessEqualThan,
            true
        ) | (
            BinaryOperator::GreaterThan | BinaryOperator::GreaterEqualThan,
            false
        )
    );
    if valid {
        Ok((number, step))
    } else {
        Err(vec!["Incompatible for direction".to_string()])
    }
}

fn update_step(statement: &ForStatement<'_>) -> Result<f64, Vec<String>> {
    let Some(Expression::UpdateExpression(update)) = statement.update.as_ref() else {
        return Err(vec!["Dynamic for update is unsupported".to_string()]);
    };
    Ok(match update.operator {
        oxc::syntax::operator::UpdateOperator::Increment => 1.0,
        oxc::syntax::operator::UpdateOperator::Decrement => -1.0,
    })
}

fn reduce_body(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let statements = match statement {
        Statement::BlockStatement(block) => &block.body,
        _ => std::slice::from_ref(statement),
    };
    for statement in statements {
        crate::reduce::reduce_statement(statement, ops, facts, next_register, next_slot, locals)?;
    }
    Ok(())
}
