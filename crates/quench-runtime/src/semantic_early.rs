use std::collections::{BTreeSet, HashSet};

use oxc::ast::ast::{BindingPattern, BindingPatternKind, Statement, VariableDeclarationKind};

pub(crate) fn validate(program: &oxc::ast::ast::Program<'_>) -> Result<(), Vec<String>> {
    validate_nested(&program.body)
}

pub(crate) fn var_declared_names(program: &oxc::ast::ast::Program<'_>) -> Vec<String> {
    var_declared_names_in(&program.body)
}

pub(crate) fn var_declared_names_in(statements: &[Statement<'_>]) -> Vec<String> {
    var_names_filtered(statements, true)
}

/// Var names hoisted out of a block in strict mode: Annex B function hoisting
/// is suppressed, so block-level function declarations stay block-scoped.
pub(crate) fn strict_var_declared_names_in(statements: &[Statement<'_>]) -> Vec<String> {
    var_names_filtered(statements, false)
}

fn var_names_filtered(statements: &[Statement<'_>], hoist_functions: bool) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_var_names(statements, &mut names, hoist_functions);
    names.into_iter().collect()
}

pub(crate) fn lexically_declared_names(program: &oxc::ast::ast::Program<'_>) -> Vec<String> {
    lexically_declared_names_in(&program.body)
}

pub(crate) fn annex_b_lexical_collisions(program: &oxc::ast::ast::Program<'_>) -> HashSet<String> {
    annex_b_lexical_collisions_in(&program.body)
}

pub(crate) fn annex_b_lexical_collisions_in(statements: &[Statement<'_>]) -> HashSet<String> {
    let mut collisions = HashSet::new();
    let visible = lexically_declared_names_in(statements);
    collect_annex_b_collisions(statements, &visible, &mut collisions);
    collisions
}

fn collect_annex_b_collisions(
    statements: &[Statement<'_>],
    visible: &[String],
    collisions: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => {
                let mut nested = visible.to_vec();
                nested.extend(lexically_declared_names_in(&block.body));
                collect_annex_b_collisions(&block.body, &nested, collisions);
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id {
                    if visible.iter().any(|name| name == identifier.name.as_str()) {
                        collisions.insert(identifier.name.to_string());
                    }
                }
            }
            Statement::IfStatement(statement) => {
                collect_annex_b_one(&statement.consequent, visible, collisions);
                if let Some(alternate) = &statement.alternate {
                    collect_annex_b_one(alternate, visible, collisions);
                }
            }
            Statement::ForInStatement(statement) => {
                collect_loop_collisions(&statement.left, &statement.body, visible, collisions);
            }
            Statement::ForOfStatement(statement) => {
                collect_loop_collisions(&statement.left, &statement.body, visible, collisions);
            }
            Statement::ForStatement(statement) => {
                let mut nested = visible.to_vec();
                extend_for_lexicals(&statement.init, &mut nested);
                collect_annex_b_collisions(
                    std::slice::from_ref(&statement.body),
                    &nested,
                    collisions,
                );
            }
            Statement::LabeledStatement(statement) => {
                collect_annex_b_one(&statement.body, visible, collisions);
            }
            Statement::WhileStatement(statement) => {
                collect_annex_b_one(&statement.body, visible, collisions);
            }
            Statement::DoWhileStatement(statement) => {
                collect_annex_b_one(&statement.body, visible, collisions);
            }
            Statement::SwitchStatement(statement) => {
                collect_switch_collisions(statement, visible, collisions);
            }
            Statement::TryStatement(statement) => {
                collect_try_collisions(statement, visible, collisions);
            }
            _ => {}
        }
    }
}

fn collect_switch_collisions(
    statement: &oxc::ast::ast::SwitchStatement<'_>,
    visible: &[String],
    collisions: &mut HashSet<String>,
) {
    let mut nested = visible.to_vec();
    for case in &statement.cases {
        nested.extend(lexically_declared_names_in(&case.consequent));
    }
    for case in &statement.cases {
        collect_annex_b_collisions(&case.consequent, &nested, collisions);
    }
}

fn collect_try_collisions(
    statement: &oxc::ast::ast::TryStatement<'_>,
    visible: &[String],
    collisions: &mut HashSet<String>,
) {
    collect_annex_b_collisions(&statement.block.body, visible, collisions);
    if let Some(handler) = &statement.handler {
        let mut nested = visible.to_vec();
        if let Some(parameter) = &handler.param {
            if !matches!(
                parameter.pattern.kind,
                BindingPatternKind::BindingIdentifier(_)
            ) {
                nested.extend(crate::binding_patterns::names(&parameter.pattern));
            }
        }
        collect_annex_b_collisions(&handler.body.body, &nested, collisions);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_annex_b_collisions(&finalizer.body, visible, collisions);
    }
}

fn collect_annex_b_one(body: &Statement<'_>, visible: &[String], collisions: &mut HashSet<String>) {
    collect_annex_b_collisions(std::slice::from_ref(body), visible, collisions);
}

fn collect_loop_collisions(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    body: &Statement<'_>,
    visible: &[String],
    collisions: &mut HashSet<String>,
) {
    let mut nested = visible.to_vec();
    extend_loop_lexicals(left, &mut nested);
    collect_annex_b_one(body, &nested, collisions);
}

fn extend_loop_lexicals(left: &oxc::ast::ast::ForStatementLeft<'_>, names: &mut Vec<String>) {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return;
    };
    if declaration.kind != VariableDeclarationKind::Var {
        names.extend(
            declaration
                .declarations
                .iter()
                .flat_map(|declarator| crate::binding_patterns::names(&declarator.id)),
        );
    }
}

fn extend_for_lexicals(
    init: &Option<oxc::ast::ast::ForStatementInit<'_>>,
    names: &mut Vec<String>,
) {
    let Some(oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration)) = init else {
        return;
    };
    if declaration.kind != VariableDeclarationKind::Var {
        names.extend(
            declaration
                .declarations
                .iter()
                .flat_map(|declarator| crate::binding_patterns::names(&declarator.id)),
        );
    }
}

pub(crate) fn lexically_declared_names_in(statements: &[Statement<'_>]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for statement in statements {
        collect_top_level_lexical_name(statement, &mut names);
    }
    names.into_iter().collect()
}

fn collect_top_level_lexical_name(statement: &Statement<'_>, names: &mut BTreeSet<String>) {
    match statement {
        Statement::VariableDeclaration(declaration)
            if declaration.kind != VariableDeclarationKind::Var =>
        {
            for declarator in &declaration.declarations {
                collect_pattern_var_names(&declarator.id, names);
            }
        }
        Statement::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                names.insert(identifier.name.to_string());
            }
        }
        _ => {}
    }
}

fn collect_var_names(
    statements: &[Statement<'_>],
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    for statement in statements {
        collect_statement_var_names(statement, names, hoist_functions);
    }
}

fn collect_statement_var_names(
    statement: &Statement<'_>,
    names: &mut BTreeSet<String>,
    hoist: bool,
) {
    if let Statement::FunctionDeclaration(function) = statement {
        if hoist {
            collect_function_name(function, names);
        }
        return;
    }
    match statement {
        Statement::VariableDeclaration(declaration) => collect_var_declaration(declaration, names),
        Statement::BlockStatement(block) => collect_var_names(&block.body, names, hoist),
        Statement::IfStatement(statement) => collect_if_var_names(statement, names, hoist),
        Statement::ForStatement(statement) => collect_for_var_names(statement, names, hoist),
        Statement::ForInStatement(statement) => collect_for_in_var_names(statement, names, hoist),
        Statement::ForOfStatement(statement) => collect_for_of_var_names(statement, names, hoist),
        Statement::SwitchStatement(statement) => collect_switch_var_names(statement, names, hoist),
        Statement::TryStatement(statement) => collect_try_var_names(statement, names, hoist),
        Statement::LabeledStatement(statement) => {
            collect_statement_var_names(&statement.body, names, hoist);
        }
        Statement::WhileStatement(statement) => {
            collect_statement_var_names(&statement.body, names, hoist);
        }
        Statement::DoWhileStatement(statement) => {
            collect_statement_var_names(&statement.body, names, hoist);
        }
        Statement::WithStatement(statement) => {
            collect_statement_var_names(&statement.body, names, hoist);
        }
        _ => {}
    }
}
include!("semantic_early_helpers.rs");
