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
    collect_annex_b_collisions(statements, &[], &mut collisions);
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

fn collect_var_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    names: &mut BTreeSet<String>,
) {
    if declaration.kind != VariableDeclarationKind::Var {
        return;
    }
    for declarator in &declaration.declarations {
        collect_pattern_var_names(&declarator.id, names);
    }
}

fn collect_pattern_var_names(pattern: &BindingPattern<'_>, names: &mut BTreeSet<String>) {
    let mut declared = HashSet::new();
    collect_pattern_names(pattern, &mut declared);
    names.extend(declared);
}

fn collect_function_name(function: &oxc::ast::ast::Function<'_>, names: &mut BTreeSet<String>) {
    if let Some(identifier) = &function.id {
        names.insert(identifier.name.to_string());
    }
}

fn collect_if_var_names(
    statement: &oxc::ast::ast::IfStatement<'_>,
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    collect_statement_var_names(&statement.consequent, names, hoist_functions);
    if let Some(alternate) = &statement.alternate {
        collect_statement_var_names(alternate, names, hoist_functions);
    }
}

fn collect_for_var_names(
    statement: &oxc::ast::ast::ForStatement<'_>,
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    if let Some(oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration)) = &statement.init
    {
        collect_var_declaration(declaration, names);
    }
    collect_statement_var_names(&statement.body, names, hoist_functions);
}

fn collect_for_in_var_names(
    statement: &oxc::ast::ast::ForInStatement<'_>,
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    collect_for_left_var_names(&statement.left, names);
    collect_statement_var_names(&statement.body, names, hoist_functions);
}

fn collect_for_of_var_names(
    statement: &oxc::ast::ast::ForOfStatement<'_>,
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    collect_for_left_var_names(&statement.left, names);
    collect_statement_var_names(&statement.body, names, hoist_functions);
}

fn collect_for_left_var_names(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    names: &mut BTreeSet<String>,
) {
    if let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left {
        collect_var_declaration(declaration, names);
    }
}

fn collect_switch_var_names(
    statement: &oxc::ast::ast::SwitchStatement<'_>,
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    for case in &statement.cases {
        collect_var_names(&case.consequent, names, hoist_functions);
    }
}

fn collect_try_var_names(
    statement: &oxc::ast::ast::TryStatement<'_>,
    names: &mut BTreeSet<String>,
    hoist_functions: bool,
) {
    collect_var_names(&statement.block.body, names, hoist_functions);
    if let Some(handler) = &statement.handler {
        collect_var_names(&handler.body.body, names, hoist_functions);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_var_names(&finalizer.body, names, hoist_functions);
    }
}

fn validate_nested(statements: &[Statement<'_>]) -> Result<(), Vec<String>> {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => validate_block(&block.body)?,
            Statement::FunctionDeclaration(function) => {
                if let Some(body) = &function.body {
                    validate_nested(&body.statements)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_block(statements: &[Statement<'_>]) -> Result<(), Vec<String>> {
    let lexical = lexical_names(statements);
    let variables = variable_names(statements);
    if let Some(name) = lexical.intersection(&variables).next() {
        return Err(vec![format!(
            "SyntaxError: block lexical declaration conflicts with var `{name}`"
        )]);
    }
    validate_nested(statements)
}

fn lexical_names(statements: &[Statement<'_>]) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in statements {
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id {
                    names.insert(identifier.name.to_string());
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(identifier) = &class.id {
                    names.insert(identifier.name.to_string());
                }
            }
            Statement::VariableDeclaration(declaration)
                if declaration.kind != VariableDeclarationKind::Var =>
            {
                collect_declaration_names(declaration, &mut names);
            }
            _ => {}
        }
    }
    names
}

fn variable_names(statements: &[Statement<'_>]) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration)
                if declaration.kind == VariableDeclarationKind::Var =>
            {
                collect_declaration_names(declaration, &mut names);
            }
            Statement::BlockStatement(block) => names.extend(variable_names(&block.body)),
            _ => {}
        }
    }
    names
}

fn collect_declaration_names(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    names: &mut HashSet<String>,
) {
    for declarator in &declaration.declarations {
        collect_pattern_names(&declarator.id, names);
    }
}

fn collect_pattern_names(pattern: &BindingPattern<'_>, names: &mut HashSet<String>) {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => {
            names.insert(identifier.name.to_string());
        }
        BindingPatternKind::AssignmentPattern(pattern) => {
            collect_pattern_names(&pattern.left, names);
        }
        BindingPatternKind::ArrayPattern(pattern) => {
            for element in pattern.elements.iter().flatten() {
                collect_pattern_names(element, names);
            }
        }
        BindingPatternKind::ObjectPattern(pattern) => {
            for property in &pattern.properties {
                collect_pattern_names(&property.value, names);
            }
        }
    }
}
