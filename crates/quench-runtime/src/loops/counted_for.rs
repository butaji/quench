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

const STATIC_REDUCTION_BOUND: usize = 1_000;

pub(crate) fn reduce_for(
    statement: &ForStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    if is_static_false_condition(statement) {
        let init = reduce_for_init(statement, facts, next_register, next_slot, locals)?;
        ops.extend(init);
        return Ok(());
    }
    let Some((name, start, limit, step)) =
        static_bounds(statement).filter(|_| !contains_loop_control(&statement.body))
    else {
        return reduce_dynamic_for(statement, ops, facts, next_register, next_slot, locals);
    };
    if !fits_static_bound(start, limit, step) {
        return reduce_dynamic_for(statement, ops, facts, next_register, next_slot, locals);
    }
    reduce_static_for(
        statement,
        (name, start, limit, step),
        ops,
        facts,
        next_register,
        next_slot,
        locals,
    )
}

pub(super) fn reduce_fragment(
    expression: Option<&Expression<'_>>,
    _parent_ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    let mut fragment = Vec::new();
    if let Some(expression) = expression {
        let register = crate::reduce::reduce_expression(
            expression,
            &mut fragment,
            facts,
            next_register,
            locals,
        )
        .ok_or_else(|| vec!["Unsupported loop fragment".to_string()])?;
        fragment.push(Op::Return { src: register });
    } else {
        fragment.push(Op::Const {
            dst: 0,
            value: Constant::Boolean(true),
        });
        fragment.push(Op::Return { src: 0 });
    }
    Ok(fragment)
}

pub(super) fn reduce_body(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let mut block_locals = locals.clone();
    let statements = match statement {
        Statement::BlockStatement(block) => &block.body,
        _ => std::slice::from_ref(statement),
    };
    let locals = if matches!(statement, Statement::BlockStatement(_)) {
        &mut block_locals
    } else {
        locals
    };
    for statement in statements {
        crate::reduce::reduce_statement(statement, ops, facts, next_register, next_slot, locals)?;
    }
    Ok(())
}

fn fits_static_bound(start: f64, limit: f64, step: f64) -> bool {
    let mut current = start;
    let mut count = 0;
    while (step > 0.0 && current < limit) || (step < 0.0 && current > limit) {
        if count >= STATIC_REDUCTION_BOUND {
            return false;
        }
        current += step;
        count += 1;
    }
    true
}

fn contains_loop_control(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::BreakStatement(_) | Statement::ContinueStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(contains_loop_control),
        Statement::IfStatement(if_statement) => {
            contains_loop_control(&if_statement.consequent)
                || if_statement
                    .alternate
                    .as_ref()
                    .is_some_and(contains_loop_control)
        }
        Statement::ForStatement(_)
        | Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::ForInStatement(_) => true,
        _ => false,
    }
}

fn is_literal_false(expression: Option<&Expression<'_>>) -> bool {
    matches!(
        expression
            .and_then(reduce_literal)
            .map(|literal| literal.op),
        Some(Constant::Boolean(false))
    )
}

fn is_static_false_condition(statement: &ForStatement<'_>) -> bool {
    is_literal_false(statement.test.as_ref())
        || (statement.test.is_none()
            && is_literal_false(
                statement
                    .init
                    .as_ref()
                    .and_then(ForStatementInit::as_expression),
            ))
}

fn static_bounds(statement: &ForStatement<'_>) -> Option<(String, f64, f64, f64)> {
    let (name, start) = numeric_init(statement.init.as_ref()).ok()?;
    let (limit, step) = numeric_test_and_update(statement, &name).ok()?;
    Some((name, start, limit, step))
}

fn reduce_static_for(
    statement: &ForStatement<'_>,
    (name, start, limit, step): (String, f64, f64, f64),
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
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
        if count >= STATIC_REDUCTION_BOUND {
            return Err(vec!["Static loop exceeds reduction bound".to_string()]);
        }
        emit_iteration(statement, current, slot, &mut context)?;
        current += step;
        count += 1;
    }
    Ok(())
}

fn reduce_dynamic_for(
    statement: &ForStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let init = reduce_for_init(statement, facts, next_register, next_slot, locals)?;
    let test = super::reduce_fragment(statement.test.as_ref(), ops, facts, next_register, locals)?;
    let var_names = body_var_names(&statement.body);
    propagate_body_vars(locals, next_slot, &var_names);
    let body = reduce_body_fragment(statement, ops, facts, next_register, next_slot, locals)?;
    let update =
        super::reduce_fragment(statement.update.as_ref(), ops, facts, next_register, locals)?;
    ops.push(Op::Loop {
        label: None,
        init,
        test,
        body,
        update,
        post_test: false,
    });
    Ok(())
}

fn body_var_names(statement: &Statement<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_body_vars(statement, &mut names);
    names
}

fn collect_body_vars(statement: &Statement<'_>, names: &mut Vec<String>) {
    match statement {
        Statement::VariableDeclaration(declaration)
            if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var =>
        {
            names.extend(declaration.declarations.iter().filter_map(|declarator| {
                declarator
                    .id
                    .get_binding_identifier()
                    .map(|id| id.name.to_string())
            }));
        }
        Statement::BlockStatement(block) => {
            for statement in &block.body {
                collect_body_vars(statement, names);
            }
        }
        Statement::TryStatement(statement) => {
            for statement in &statement.block.body {
                collect_body_vars(statement, names);
            }
            if let Some(handler) = &statement.handler {
                for statement in &handler.body.body {
                    collect_body_vars(statement, names);
                }
            }
            if let Some(finalizer) = &statement.finalizer {
                for statement in &finalizer.body {
                    collect_body_vars(statement, names);
                }
            }
        }
        _ => {}
    }
}

fn propagate_body_vars(locals: &mut HashMap<String, u16>, next_slot: &mut u16, names: &[String]) {
    for name in names {
        if !locals.contains_key(name) {
            locals.insert(name.clone(), *next_slot);
            *next_slot = next_slot.saturating_add(1);
        }
    }
}

fn reduce_for_init(
    statement: &ForStatement<'_>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    let mut fragment = Vec::new();
    match statement.init.as_ref() {
        Some(ForStatementInit::VariableDeclaration(declaration)) => {
            crate::reduce::reduce_declaration(
                declaration,
                &mut fragment,
                facts,
                next_register,
                next_slot,
                locals,
            )?;
        }
        Some(init) => {
            let expression = init
                .as_expression()
                .ok_or_else(|| vec!["Unsupported for initializer".to_string()])?;
            crate::reduce::reduce_expression(
                expression,
                &mut fragment,
                facts,
                next_register,
                locals,
            )
            .ok_or_else(|| vec!["Unsupported for initializer".to_string()])?;
        }
        None => {}
    }
    crate::reduce_support::finish_program(fragment, None)
}

fn reduce_body_fragment(
    statement: &ForStatement<'_>,
    _parent_ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    let mut fragment = Vec::new();
    super::reduce_body(
        &statement.body,
        &mut fragment,
        facts,
        next_register,
        next_slot,
        locals,
    )?;
    Ok(fragment)
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
    super::reduce_body(
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
