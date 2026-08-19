use oxc::ast::ast::{Statement, TryStatement, VariableDeclarationKind};

pub(crate) fn validate_try(
    statement: &TryStatement<'_>,
    nested: impl Fn(&[Statement<'_>]) -> Result<(), Vec<String>>,
) -> Result<(), Vec<String>> {
    validate_catch_clause(statement)?;
    nested(&statement.block.body)?;
    if let Some(handler) = &statement.handler {
        nested(&handler.body.body)?;
    }
    if let Some(finalizer) = &statement.finalizer {
        nested(&finalizer.body)?;
    }
    Ok(())
}

fn validate_catch_clause(statement: &TryStatement<'_>) -> Result<(), Vec<String>> {
    let Some(handler) = &statement.handler else {
        return Ok(());
    };
    let Some(parameter) = &handler.param else {
        return Ok(());
    };
    let bound = crate::binding_patterns::names(&parameter.pattern);
    let lexical = catch_block_lexical_names(&handler.body.body);
    if let Some(name) = bound
        .iter()
        .find(|name| lexical.iter().any(|item| item == *name))
    {
        return Err(vec![format!(
            "SyntaxError: catch parameter `{name}` redeclared in the catch block"
        )]);
    }
    Ok(())
}

fn catch_block_lexical_names(statements: &[Statement<'_>]) -> Vec<String> {
    let mut names = Vec::new();
    for statement in statements {
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id {
                    names.push(identifier.name.to_string());
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(identifier) = &class.id {
                    names.push(identifier.name.to_string());
                }
            }
            Statement::VariableDeclaration(declaration)
                if declaration.kind != VariableDeclarationKind::Var =>
            {
                names.extend(
                    declaration
                        .declarations
                        .iter()
                        .flat_map(|declarator| crate::binding_patterns::names(&declarator.id)),
                );
            }
            _ => {}
        }
    }
    names
}
