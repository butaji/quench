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
    ast::ast::{Expression, PrivateInExpression, Statement, VariableDeclarationKind},
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
    let dst = crate::reduce_support::emit_undefined(ops, next_register);
    let then_ops = reduce_if_branch(&statement.consequent, dst, facts, locals, next_slot)?;
    let else_ops = statement
        .alternate
        .as_ref()
        .map(|alternate| reduce_if_branch(alternate, dst, facts, locals, next_slot))
        .transpose()?
        .unwrap_or_default();
    push_branch(ops, condition, then_ops, else_ops)?;
    Ok(Some(dst))
}

fn reduce_if_branch(
    statement: &Statement<'_>,
    dst: u16,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    next_slot: &mut u16,
) -> Result<Vec<Op>, Vec<String>> {
    if should_skip_if_function(statement, facts) {
        return Ok(Vec::new());
    }
    let (mut ops, last) = crate::branch::reduce(statement, facts, locals, next_slot)?;
    crate::reduce_support::seal_completion(&mut ops, dst, last);
    Ok(ops)
}

fn should_skip_if_function(statement: &Statement<'_>, facts: &ProgramDb) -> bool {
    let Statement::FunctionDeclaration(function) = statement else {
        return false;
    };
    function.id.as_ref().is_some_and(|identifier| {
        let name = identifier.name.as_str();
        facts.eval_var_barrier.iter().any(|bound| bound == name)
            || facts.eval_formals.iter().any(|bound| bound == name)
    })
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
        return Ok(Some(crate::reduce_support::emit_undefined(
            ops,
            next_register,
        )));
    };
    if should_skip_if_function(selected, facts) {
        return Ok(Some(crate::reduce_support::emit_undefined(
            ops,
            next_register,
        )));
    }
    match selected {
        Statement::EmptyStatement(_) => Ok(Some(crate::reduce_support::emit_undefined(
            ops,
            next_register,
        ))),
        Statement::BlockStatement(_) => {
            // Always emit the if's V as a dst register holding the
            // block's last value (or undefined for an empty block).
            // The plain `reduce_statement` would return None for an
            // empty block, leaving the body's sequence to inherit the
            // previous V (which is wrong per ES 13.6.2 step 5.d).
            let dst = crate::reduce_support::emit_undefined(ops, next_register);
            let last = crate::reduce::reduce_statement(
                selected,
                ops,
                facts,
                next_register,
                next_slot,
                locals,
            )?;
            crate::reduce_support::seal_completion(ops, dst, last);
            Ok(Some(dst))
        }
        Statement::ExpressionStatement(expression) => {
            reduce_expression_statement(&expression.expression, ops, facts, next_register, locals)
                .map(Some)
        }
        Statement::VariableDeclaration(declaration) => {
            reduce_declaration(declaration, ops, facts, next_register, next_slot, locals)?;
            Ok(None)
        }
        Statement::FunctionDeclaration(function) => {
            let mut if_locals = locals.clone();
            crate::blocks::prepare_block_functions(
                std::slice::from_ref(selected),
                &mut if_locals,
                next_slot,
                ops,
            );
            crate::reduce::reduce_function_declaration(
                function,
                ops,
                facts,
                next_register,
                next_slot,
                &mut if_locals,
            )?;
            Ok(None)
        }
        Statement::IfStatement(_)
        | Statement::WithStatement(_)
        | Statement::TryStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::ForStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::LabeledStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_) => {
            crate::reduce::reduce_statement(selected, ops, facts, next_register, next_slot, locals)
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
        allocate_pattern_slots(&declarator.id, declaration.kind, next_slot, locals, facts);
        if declaration.kind != VariableDeclarationKind::Var {
            crate::using_scope::mark_binding_tdz(&declarator.id, ops, locals);
        }
        if declaration.kind == VariableDeclarationKind::Var && declarator.init.is_none() {
            continue;
        }
        facts.inferred_name = binding_inferred_name(&declarator.id, declarator.init.as_ref());
        let register = match declarator.init.as_ref() {
            Some(init) => reduce_expression(init, ops, facts, next_register, locals),
            None => Some(crate::reduce_support::emit_undefined(ops, next_register)),
        };
        facts.inferred_name = None;
        let Some(register) = register else {
            return Err(vec!["Unsupported variable initializer".to_string()]);
        };
        infer_declaration_name(&declarator.id, declarator.init.as_ref(), register, ops);
        crate::binding_patterns::bind(&declarator.id, register, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported binding pattern".to_string()])?;
        crate::using_scope::register_resource(
            declaration.kind,
            register,
            ops,
            next_register,
            locals,
        );
        crate::using_scope::mark_binding_immutable(declaration.kind, &declarator.id, ops, locals);
    }
    Ok(())
}

fn binding_inferred_name(
    pattern: &oxc::ast::ast::BindingPattern<'_>,
    initializer: Option<&Expression<'_>>,
) -> Option<String> {
    let initializer = initializer?;
    if !crate::binding_patterns::anonymous_function_definition(initializer) {
        return None;
    }
    let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) = &pattern.kind else {
        return None;
    };
    Some(identifier.name.to_string())
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
    facts: &ProgramDb,
) {
    for name in crate::binding_patterns::names(pattern) {
        let slot = declaration_slot(kind, &name, next_slot, locals, facts);
        locals.insert(name, slot);
    }
}

fn declaration_slot(
    kind: VariableDeclarationKind,
    name: &str,
    next_slot: &mut u16,
    locals: &HashMap<String, u16>,
    facts: &ProgramDb,
) -> u16 {
    if kind != VariableDeclarationKind::Var {
        let marker = format!("\0lexical-predeclared:{name}");
        if let Some(slot) = locals.get(&marker) {
            return *slot;
        }
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        return slot;
    }
    if let Some(slot) = locals.get(name) {
        if facts.function_name_slot != Some(*slot) {
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
    match expression {
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
        .or_else(|| reduce_private_in(expression, ops, facts, next_register, locals))
}

fn reduce_private_in(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let Expression::PrivateInExpression(boxed) = expression else {
        return None;
    };
    let PrivateInExpression { left, right, .. } = boxed.as_ref();
    let dst = take_register(next_register);
    let name = facts.private_name(left.span)?;
    let object = reduce_expression(right, ops, facts, next_register, locals)?;
    ops.push(Op::HasPrivate { dst, object, name });
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

fn reduce_import_expression(
    import: &oxc::ast::ast::ImportExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    // Dynamic imports have no module graph in this runtime, so eagerly
    // evaluate the specifier/options for side effects and return a Promise.
    let specifier = reduce_expression(&import.source, ops, facts, next_register, locals)?;
    for argument in &import.arguments {
        reduce_expression(argument, ops, facts, next_register, locals)?;
    }
    let deferred = import.phase == Some(oxc::ast::ast::ImportPhase::Defer);
    let namespace = take_register(next_register);
    ops.push(Op::DynamicImport {
        dst: namespace,
        specifier,
        deferred,
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
        args: vec![namespace],
        spreads: vec![false],
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
    let dst = take_register(next_register);
    if let Expression::PrivateFieldExpression(member) = &binary.left {
        let name = facts.private_name(member.field.span)?;
        let object = reduce_expression(&member.object, ops, facts, next_register, locals)?;
        ops.push(Op::HasPrivate { dst, object, name });
        return Some(dst);
    }
    // For `#field in obj`, OXC may package the LHS as a `PrivateIdentifier`
    // wrapped in an `Identifier` whose name carries the `#`. Fall through
    // to the dynamic path which will then fail; the structural path above
    // matches the field-on-object form.
    let _ = binary;
    let key = reduce_expression(&binary.left, ops, facts, next_register, locals)?;
    let object = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
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

include!("reduce_expressions_rest.rs");
