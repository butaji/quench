fn hoisted_try_locals(
    statement: &oxc::ast::ast::TryStatement<'_>,
    locals: &HashMap<String, u16>,
) -> (HashMap<String, u16>, u16) {
    let mut result = locals.clone();
    let mut next_slot = crate::reduce_support::register_base(&result);
    collect_statements_into(&statement.block.body, &mut result, &mut next_slot);
    if let Some(handler) = &statement.handler {
        collect_statements_into(&handler.body.body, &mut result, &mut next_slot);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_statements_into(&finalizer.body, &mut result, &mut next_slot);
    }
    (result, next_slot)
}

fn collect_statements_into(
    statements: &[Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        collect_statement_vars(statement, locals, next_slot);
    }
}

fn collect_statement_vars(
    statement: &Statement<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    match statement {
        Statement::VariableDeclaration(declaration) => {
            collect_var_declaration(declaration, locals, next_slot);
        }
        Statement::BlockStatement(block) => collect_statements_into(&block.body, locals, next_slot),
        Statement::IfStatement(statement) => {
            collect_statement_vars(&statement.consequent, locals, next_slot);
            if let Some(alternate) = &statement.alternate {
                collect_statement_vars(alternate, locals, next_slot);
            }
        }
        Statement::LabeledStatement(_)
        | Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::ForStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_) => collect_nested_body_vars(statement, locals, next_slot),
        Statement::SwitchStatement(statement) => {
            for case in &statement.cases {
                collect_statements_into(&case.consequent, locals, next_slot);
            }
        }
        Statement::TryStatement(statement) => collect_try_parts(statement, locals, next_slot),
        _ => {}
    }
}

fn collect_nested_body_vars(
    statement: &Statement<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    match statement {
        Statement::LabeledStatement(statement) => {
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::WhileStatement(statement) => {
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::DoWhileStatement(statement) => {
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::ForStatement(statement) => {
            if let Some(init) = &statement.init {
                collect_for_init_vars(init, locals, next_slot);
            }
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::ForInStatement(statement) => {
            collect_for_left_vars(&statement.left, locals, next_slot);
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        Statement::ForOfStatement(statement) => {
            collect_for_left_vars(&statement.left, locals, next_slot);
            collect_statement_vars(&statement.body, locals, next_slot);
        }
        _ => {}
    }
}

fn collect_try_parts(
    statement: &oxc::ast::ast::TryStatement<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    collect_statements_into(&statement.block.body, locals, next_slot);
    if let Some(handler) = &statement.handler {
        collect_statements_into(&handler.body.body, locals, next_slot);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_statements_into(&finalizer.body, locals, next_slot);
    }
}

fn collect_for_init_vars(
    init: &oxc::ast::ast::ForStatementInit<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    if let oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration) = init {
        collect_var_declaration(declaration, locals, next_slot);
    }
}

fn collect_for_left_vars(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    if let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left {
        collect_var_declaration(declaration, locals, next_slot);
    }
}

fn collect_var_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    if declaration.kind != VariableDeclarationKind::Var {
        return;
    }
    for declarator in &declaration.declarations {
        if let Some(identifier) = declarator.id.get_binding_identifier() {
            insert_hoisted_var(identifier.name.as_str(), locals, next_slot);
        }
    }
}

fn insert_hoisted_var(name: &str, locals: &mut HashMap<String, u16>, next_slot: &mut u16) {
    if locals.contains_key(name) {
        return;
    }
    locals.insert(name.to_string(), *next_slot);
    *next_slot = next_slot.saturating_add(1);
}
