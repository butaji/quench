use oxc::ast::ast::{
    Statement, VariableDeclaration, VariableDeclarationKind,
};

const USING_SYNTAX: &str = "SyntaxError: invalid using declaration";

pub(crate) fn validate(program: &oxc::ast::ast::Program<'_>) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if program.source_type.is_script() {
        reject_list_using(&program.body, &mut errors);
    }
    walk_statements(&program.body, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn walk_statements(statements: &[Statement<'_>], errors: &mut Vec<String>) {
    for statement in statements {
        walk_statement(statement, errors);
    }
}

fn walk_statement(statement: &Statement<'_>, errors: &mut Vec<String>) {
    match statement {
        Statement::BlockStatement(block) => walk_statements(&block.body, errors),
        Statement::SwitchStatement(statement) => {
            for case in &statement.cases {
                reject_list_using(&case.consequent, errors);
                walk_statements(&case.consequent, errors);
            }
        }
        Statement::IfStatement(statement) => {
            reject_lone_using(&statement.consequent, errors);
            walk_statement(&statement.consequent, errors);
            if let Some(alternate) = &statement.alternate {
                reject_lone_using(alternate, errors);
                walk_statement(alternate, errors);
            }
        }
        Statement::WhileStatement(statement) => {
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::DoWhileStatement(statement) => {
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::LabeledStatement(statement) => {
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::WithStatement(statement) => {
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::ForStatement(statement) => {
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::ForInStatement(statement) => {
            reject_for_left_using(&statement.left, errors);
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::ForOfStatement(statement) => {
            reject_lone_using(&statement.body, errors);
            walk_statement(&statement.body, errors);
        }
        Statement::TryStatement(statement) => {
            walk_statements(&statement.block.body, errors);
            if let Some(handler) = &statement.handler {
                walk_statements(&handler.body.body, errors);
            }
            if let Some(finalizer) = &statement.finalizer {
                walk_statements(&finalizer.body, errors);
            }
        }
        Statement::FunctionDeclaration(function) => walk_function(function, errors),
        Statement::ClassDeclaration(class) => walk_class(class, errors),
        Statement::ExpressionStatement(statement) => walk_expression(&statement.expression, errors),
        Statement::ReturnStatement(statement) => {
            if let Some(expression) = &statement.argument {
                walk_expression(expression, errors);
            }
        }
        Statement::VariableDeclaration(declaration) => {
            reject_using_without_init(declaration, errors);
            for declarator in &declaration.declarations {
                if let Some(init) = &declarator.init {
                    walk_expression(init, errors);
                }
            }
        }
        _ => {}
    }
}

fn walk_function(function: &oxc::ast::ast::Function<'_>, errors: &mut Vec<String>) {
    if let Some(body) = &function.body {
        walk_statements(&body.statements, errors);
    }
}

fn walk_class(class: &oxc::ast::ast::Class<'_>, errors: &mut Vec<String>) {
    for element in &class.body.body {
        let oxc::ast::ast::ClassElement::MethodDefinition(method) = element else {
            continue;
        };
        walk_function(&method.value, errors);
    }
}

fn walk_expression(expression: &oxc::ast::ast::Expression<'_>, errors: &mut Vec<String>) {
    match expression {
        oxc::ast::ast::Expression::FunctionExpression(function) => {
            walk_function(function, errors);
        }
        oxc::ast::ast::Expression::ArrowFunctionExpression(arrow) => {
            walk_statements(&arrow.body.statements, errors);
        }
        oxc::ast::ast::Expression::ParenthesizedExpression(value) => {
            walk_expression(&value.expression, errors);
        }
        oxc::ast::ast::Expression::CallExpression(call) => {
            walk_expression(&call.callee, errors);
        }
        _ => {}
    }
}

fn reject_list_using(statements: &[Statement<'_>], errors: &mut Vec<String>) {
    if statements.iter().any(is_using_statement) {
        errors.push(USING_SYNTAX.to_string());
    }
}

fn reject_lone_using(statement: &Statement<'_>, errors: &mut Vec<String>) {
    if is_using_statement(statement) {
        errors.push(USING_SYNTAX.to_string());
    }
}

fn reject_using_without_init(
    declaration: &VariableDeclaration<'_>,
    errors: &mut Vec<String>,
) {
    if !is_using_kind(declaration.kind) {
        return;
    }
    if declaration.declarations.iter().any(|item| item.init.is_none()) {
        errors.push(USING_SYNTAX.to_string());
    }
}

fn reject_for_left_using(left: &oxc::ast::ast::ForStatementLeft<'_>, errors: &mut Vec<String>) {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return;
    };
    if is_using_kind(declaration.kind) {
        errors.push(USING_SYNTAX.to_string());
    }
}

fn is_using_statement(statement: &Statement<'_>) -> bool {
    let Statement::VariableDeclaration(declaration) = statement else {
        return false;
    };
    is_using_kind(declaration.kind)
}

fn is_using_kind(kind: VariableDeclarationKind) -> bool {
    matches!(
        kind,
        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
    )
}
