
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
            Statement::SwitchStatement(statement) => validate_case_block(statement)?,
            Statement::FunctionDeclaration(function) => validate_function(function)?,
            Statement::ExpressionStatement(statement) => {
                validate_expression(&statement.expression)?;
            }
            Statement::WhileStatement(statement) => validate_loop_body(&statement.body)?,
            Statement::DoWhileStatement(statement) => validate_loop_body(&statement.body)?,
            Statement::ForStatement(statement) => validate_loop_body(&statement.body)?,
            Statement::ForInStatement(statement) => validate_loop_body(&statement.body)?,
            Statement::ForOfStatement(statement) => validate_loop_body(&statement.body)?,
            Statement::LabeledStatement(statement) => {
                validate_nested(std::slice::from_ref(&statement.body))?;
            }
            Statement::TryStatement(statement) => {
                crate::semantic_catch::validate_try(statement, validate_nested)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_loop_body(body: &Statement<'_>) -> Result<(), Vec<String>> {
    if is_labelled_function(body) {
        return Err(vec![
            "SyntaxError: labelled functions are not allowed as loop bodies".to_string(),
        ]);
    }
    validate_nested(std::slice::from_ref(body))
}

fn is_labelled_function(statement: &Statement<'_>) -> bool {
    let Statement::LabeledStatement(labelled) = statement else {
        return false;
    };
    unwraps_to_function(&labelled.body)
}

fn unwraps_to_function(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::FunctionDeclaration(_) => true,
        Statement::LabeledStatement(labelled) => unwraps_to_function(&labelled.body),
        _ => false,
    }
}

fn validate_function(function: &oxc::ast::ast::Function<'_>) -> Result<(), Vec<String>> {
    function
        .body
        .as_ref()
        .map_or(Ok(()), |body| validate_nested(&body.statements))
}

fn validate_expression(expression: &oxc::ast::ast::Expression<'_>) -> Result<(), Vec<String>> {
    match expression {
        oxc::ast::ast::Expression::FunctionExpression(function) => validate_function(function),
        oxc::ast::ast::Expression::ParenthesizedExpression(value) => {
            validate_expression(&value.expression)
        }
        oxc::ast::ast::Expression::CallExpression(call) => validate_expression(&call.callee),
        _ => Ok(()),
    }
}

fn validate_case_block(statement: &oxc::ast::ast::SwitchStatement<'_>) -> Result<(), Vec<String>> {
    let mut lexical = HashSet::new();
    let mut variables = HashSet::new();
    for case in &statement.cases {
        lexical.extend(lexical_names(&case.consequent));
        variables.extend(variable_names(&case.consequent));
        validate_nested(&case.consequent)?;
    }
    if let Some(name) = lexical.intersection(&variables).next() {
        return Err(vec![format!(
            "SyntaxError: block lexical declaration conflicts with var `{name}`"
        )]);
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