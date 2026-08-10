use std::collections::{BTreeSet, HashSet};

use oxc::ast::ast::{BindingPattern, BindingPatternKind, Statement, VariableDeclarationKind};

pub(crate) fn validate(program: &oxc::ast::ast::Program<'_>) -> Result<(), Vec<String>> {
    validate_nested(&program.body)
}

pub(crate) fn var_declared_names(program: &oxc::ast::ast::Program<'_>) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_var_names(&program.body, &mut names);
    names.into_iter().collect()
}

fn collect_var_names(statements: &[Statement<'_>], names: &mut BTreeSet<String>) {
    for statement in statements {
        collect_statement_var_names(statement, names);
    }
}

fn collect_statement_var_names(statement: &Statement<'_>, names: &mut BTreeSet<String>) {
    match statement {
        Statement::VariableDeclaration(declaration) => collect_var_declaration(declaration, names),
        Statement::FunctionDeclaration(function) => collect_function_name(function, names),
        Statement::BlockStatement(block) => collect_var_names(&block.body, names),
        Statement::IfStatement(statement) => collect_if_var_names(statement, names),
        Statement::LabeledStatement(statement) => {
            collect_statement_var_names(&statement.body, names);
        }
        Statement::WhileStatement(statement) => {
            collect_statement_var_names(&statement.body, names);
        }
        Statement::DoWhileStatement(statement) => {
            collect_statement_var_names(&statement.body, names);
        }
        Statement::ForStatement(statement) => collect_for_var_names(statement, names),
        Statement::ForInStatement(statement) => collect_for_in_var_names(statement, names),
        Statement::ForOfStatement(statement) => collect_for_of_var_names(statement, names),
        Statement::SwitchStatement(statement) => collect_switch_var_names(statement, names),
        Statement::TryStatement(statement) => collect_try_var_names(statement, names),
        Statement::WithStatement(statement) => collect_statement_var_names(&statement.body, names),
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

fn collect_if_var_names(statement: &oxc::ast::ast::IfStatement<'_>, names: &mut BTreeSet<String>) {
    collect_statement_var_names(&statement.consequent, names);
    if let Some(alternate) = &statement.alternate {
        collect_statement_var_names(alternate, names);
    }
}

fn collect_for_var_names(
    statement: &oxc::ast::ast::ForStatement<'_>,
    names: &mut BTreeSet<String>,
) {
    if let Some(oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration)) = &statement.init
    {
        collect_var_declaration(declaration, names);
    }
    collect_statement_var_names(&statement.body, names);
}

fn collect_for_in_var_names(
    statement: &oxc::ast::ast::ForInStatement<'_>,
    names: &mut BTreeSet<String>,
) {
    collect_for_left_var_names(&statement.left, names);
    collect_statement_var_names(&statement.body, names);
}

fn collect_for_of_var_names(
    statement: &oxc::ast::ast::ForOfStatement<'_>,
    names: &mut BTreeSet<String>,
) {
    collect_for_left_var_names(&statement.left, names);
    collect_statement_var_names(&statement.body, names);
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
) {
    for case in &statement.cases {
        collect_var_names(&case.consequent, names);
    }
}

fn collect_try_var_names(
    statement: &oxc::ast::ast::TryStatement<'_>,
    names: &mut BTreeSet<String>,
) {
    collect_var_names(&statement.block.body, names);
    if let Some(handler) = &statement.handler {
        collect_var_names(&handler.body.body, names);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_var_names(&finalizer.body, names);
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
