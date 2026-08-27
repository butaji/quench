use std::collections::HashMap;

use oxc::ast::ast::{Expression, ForStatement, ForStatementInit, Statement};

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
) -> Result<Option<u16>, Vec<String>> {
    let dst = crate::reduce_support::emit_undefined(ops, next_register);
    if is_static_false_condition(statement) {
        let init = reduce_for_init(statement, facts, next_register, next_slot, locals)?;
        ops.extend(init);
        return Ok(Some(dst));
    }
    reduce_dynamic_for(statement, ops, facts, next_register, next_slot, locals, dst)
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

fn reduce_dynamic_for(
    statement: &ForStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    dst: u16,
) -> Result<Option<u16>, Vec<String>> {
    let await_using = crate::using_scope::for_init_kind(statement.init.as_ref())
        .is_some_and(|kind| kind == oxc::ast::ast::VariableDeclarationKind::AwaitUsing);
    let stack = crate::using_scope::for_init_kind(statement.init.as_ref())
        .map(|_| crate::using_scope::reserve_slot(locals, next_slot));
    if let Some(stack) = stack {
        crate::using_scope::emit_create(ops, stack, await_using, next_register);
    }
    let lexical_names = statement
        .init
        .as_ref()
        .and_then(|init| match init {
            ForStatementInit::VariableDeclaration(declaration)
                if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var =>
            {
                Some(
                    declaration
                        .declarations
                        .iter()
                        .flat_map(|declarator| crate::binding_patterns::names(&declarator.id))
                        .collect::<Vec<_>>(),
                )
            }
            _ => None,
        })
        .unwrap_or_default();
    let saved_names = lexical_names
        .iter()
        .map(|name| (name, locals.get(name).copied()))
        .collect::<Vec<_>>();
    let saved_predeclared = lexical_names
        .iter()
        .map(|name| {
            (
                name,
                locals
                    .get(&format!("\0lexical-predeclared:{name}"))
                    .copied(),
            )
        })
        .collect::<Vec<_>>();
    for name in &lexical_names {
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(name.clone(), slot);
        locals.insert(format!("\0lexical-predeclared:{name}"), slot);
    }
    let init = reduce_for_init(statement, facts, next_register, next_slot, locals)?;
    let test = super::reduce_fragment(statement.test.as_ref(), ops, facts, next_register, locals)?;
    let var_names = body_var_names(&statement.body);
    propagate_body_vars(locals, next_slot, &var_names);
    let mut body_slots = Vec::new();
    let body = reduce_body_fragment_with_slots(
        statement,
        ops,
        facts,
        next_register,
        next_slot,
        locals,
        dst,
        &mut body_slots,
    )?;
    let mut update =
        super::reduce_fragment(statement.update.as_ref(), ops, facts, next_register, locals)?;
    if statement.init.as_ref().is_some_and(|init| {
        matches!(
            init,
            ForStatementInit::VariableDeclaration(declaration)
                if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Const
        )
    }) {
        for name in &lexical_names {
            if let Some(&slot) = locals.get(name) {
                update.insert(0, Op::MarkImmutable { slot });
            }
        }
    }
    let [init, test, body, update] =
        crate::machine::FunctionCode::pending_many(vec![init, test, body, update])
            .try_into()
            .expect("four loop bodies");
    let mut per_iteration = lexical_names
        .iter()
        .filter_map(|name| locals.get(name.as_str()).copied())
        .collect::<Vec<_>>();
    for slot in body_slots {
        if !per_iteration.contains(&slot) {
            per_iteration.push(slot);
        }
    }
    ops.push(Op::Loop {
        label: None,
        init,
        test,
        body,
        update,
        post_test: false,
        dst,
        per_iteration,
    });
    if let Some(stack) = stack {
        let loop_op = ops.pop().ok_or_else(|| vec!["missing loop".to_string()])?;
        ops.extend(crate::using_scope::wrap(
            vec![loop_op],
            stack,
            await_using,
            next_register,
        )?);
    }
    for (name, slot) in saved_names {
        match slot {
            Some(slot) => {
                locals.insert(name.clone(), slot);
            }
            None => {
                locals.remove(name.as_str());
            }
        }
    }
    for (name, slot) in saved_predeclared {
        let marker = format!("\0lexical-predeclared:{name}");
        match slot {
            Some(slot) => {
                locals.insert(marker, slot);
            }
            None => {
                locals.remove(&marker);
            }
        }
    }
    Ok(Some(dst))
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
    dst: u16,
) -> Result<Vec<Op>, Vec<String>> {
    reduce_body_fragment_with_slots(
        statement,
        _parent_ops,
        facts,
        next_register,
        next_slot,
        locals,
        dst,
        &mut Vec::new(),
    )
}

fn reduce_body_fragment_with_slots(
    statement: &ForStatement<'_>,
    _parent_ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    dst: u16,
    body_slots: &mut Vec<u16>,
) -> Result<Vec<Op>, Vec<String>> {
    let mut fragment = Vec::new();
    let barrier_len = facts.eval_var_barrier.len();
    extend_for_barrier(statement, facts);
    let result = crate::loops::reduce_loop_body_slots(
        &statement.body,
        &mut fragment,
        facts,
        next_register,
        next_slot,
        locals,
        dst,
        body_slots,
    );
    facts.eval_var_barrier.truncate(barrier_len);
    result.map(|_| fragment)
}

fn extend_for_barrier(statement: &ForStatement<'_>, facts: &mut ProgramDb) {
    let Some(ForStatementInit::VariableDeclaration(declaration)) = &statement.init else {
        return;
    };
    if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var {
        facts.eval_var_barrier.extend(
            declaration
                .declarations
                .iter()
                .flat_map(|declarator| crate::binding_patterns::names(&declarator.id)),
        );
    }
}
