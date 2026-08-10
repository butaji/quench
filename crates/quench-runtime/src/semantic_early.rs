use std::collections::HashSet;

use oxc::ast::ast::{BindingPattern, BindingPatternKind, Statement, VariableDeclarationKind};

pub(crate) fn validate(program: &oxc::ast::ast::Program<'_>) -> Result<(), Vec<String>> {
    validate_nested(&program.body)
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
