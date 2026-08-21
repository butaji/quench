fn module_var_names(statement: &oxc::ast::ast::Statement<'_>) -> Vec<String> {
    let declaration = match statement {
        oxc::ast::ast::Statement::VariableDeclaration(declaration) => declaration,
        oxc::ast::ast::Statement::ExportNamedDeclaration(export) => {
            match &export.declaration {
                Some(oxc::ast::ast::Declaration::VariableDeclaration(declaration)) => declaration,
                _ => return Vec::new(),
            }
        }
        _ => return Vec::new(),
    };
    if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var {
        return Vec::new();
    }
    declaration
        .declarations
        .iter()
        .flat_map(|declarator| crate::binding_patterns::names(&declarator.id))
        .collect()
}

pub(crate) fn predeclare_lexicals(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        for name in lexical_bound_names(statement) {
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(name.clone(), slot);
            locals.insert(format!("\0lexical-predeclared:{name}"), slot);
        }
    }
}

pub(crate) fn switch_var_names(statement: &oxc::ast::ast::SwitchStatement<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for case in &statement.cases {
        collect_block_var_names(&case.consequent, &mut names);
    }
    names
}

fn collect_declared_names(statement: &oxc::ast::ast::Statement<'_>, names: &mut Vec<String>) {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                names.push(identifier.name.to_string());
            }
        }
        oxc::ast::ast::Statement::SwitchStatement(statement) => {
            names.extend(switch_var_names(statement));
        }
        oxc::ast::ast::Statement::VariableDeclaration(declaration)
            if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var =>
        {
            collect_declaration_names(declaration, names);
        }
        oxc::ast::ast::Statement::BlockStatement(block) => {
            collect_block_var_names(&block.body, names);
        }
        oxc::ast::ast::Statement::ForStatement(statement) => {
            if let Some(oxc::ast::ast::ForStatementInit::VariableDeclaration(declaration)) =
                &statement.init
            {
                collect_declaration_names(declaration, names);
            }
            collect_declared_names(&statement.body, names);
        }
        oxc::ast::ast::Statement::IfStatement(statement) => {
            collect_nested_declared_names(&statement.consequent, names);
            if let Some(alternate) = &statement.alternate {
                collect_nested_declared_names(alternate, names);
            }
        }
        oxc::ast::ast::Statement::WhileStatement(statement) => {
            collect_declared_names(&statement.body, names);
        }
        oxc::ast::ast::Statement::DoWhileStatement(statement) => {
            collect_declared_names(&statement.body, names);
        }
        oxc::ast::ast::Statement::TryStatement(statement) => {
            collect_try_declared_names(statement, names);
        }
        oxc::ast::ast::Statement::ForInStatement(statement) => {
            collect_for_left_declared_names(&statement.left, names);
            collect_declared_names(&statement.body, names);
        }
        oxc::ast::ast::Statement::ForOfStatement(statement) => {
            collect_for_left_declared_names(&statement.left, names);
            collect_declared_names(&statement.body, names);
        }
        _ => {}
    }
}

fn collect_try_declared_names(
    statement: &oxc::ast::ast::TryStatement<'_>,
    names: &mut Vec<String>,
) {
    collect_statement_names(&statement.block.body, names);
    if let Some(handler) = &statement.handler {
        collect_statement_names(&handler.body.body, names);
    }
    if let Some(finalizer) = &statement.finalizer {
        collect_statement_names(&finalizer.body, names);
    }
}

fn collect_for_left_declared_names(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    names: &mut Vec<String>,
) {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return;
    };
    if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var {
        return;
    }
    collect_declaration_names(declaration, names);
}

fn collect_nested_declared_names(
    statement: &oxc::ast::ast::Statement<'_>,
    names: &mut Vec<String>,
) {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(_) => {}
        oxc::ast::ast::Statement::BlockStatement(block) => {
            collect_block_var_names(&block.body, names)
        }
        oxc::ast::ast::Statement::IfStatement(statement) => {
            collect_nested_declared_names(&statement.consequent, names);
            if let Some(alternate) = &statement.alternate {
                collect_nested_declared_names(alternate, names);
            }
        }
        _ => collect_declared_names(statement, names),
    }
}

fn collect_block_var_names(statements: &[oxc::ast::ast::Statement<'_>], names: &mut Vec<String>) {
    for statement in statements {
        if !matches!(statement, oxc::ast::ast::Statement::FunctionDeclaration(_)) {
            collect_declared_names(statement, names);
        }
    }
}

fn collect_statement_names(statements: &[oxc::ast::ast::Statement<'_>], names: &mut Vec<String>) {
    for statement in statements {
        collect_declared_names(statement, names);
    }
}

fn collect_declaration_names(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    names: &mut Vec<String>,
) {
    for declarator in &declaration.declarations {
        if let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
            &declarator.id.kind
        {
            names.push(identifier.name.to_string());
        }
    }
}

fn reserve(name: &str, locals: &mut HashMap<String, u16>, next_slot: &mut u16) {
    if !locals.contains_key(name) {
        locals.insert(name.to_string(), *next_slot);
        *next_slot = next_slot.saturating_add(1);
    }
}

/// Mirror script `var` and function bindings onto the actual script global.
pub(crate) fn mirror_script_bindings(
    statement: &oxc::ast::ast::Statement<'_>,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    _next_register: &mut u16,
) {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                create_global_binding(identifier.name.as_str(), locals, ops, true);
            }
        }
        oxc::ast::ast::Statement::VariableDeclaration(declaration)
            if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var =>
        {
            for declarator in &declaration.declarations {
                if let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
                    &declarator.id.kind
                {
                    create_global_binding(identifier.name.as_str(), locals, ops, false);
                }
            }
        }
        _ => {}
    }
}

fn create_global_binding(
    name: &str,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    function: bool,
) {
    if !locals.contains_key(SCRIPT_THIS_SLOT) {
        return;
    }
    let Some(slot) = locals.get(name) else { return };
    let op = if function {
        Op::CreateGlobalFunction {
            name: name.to_string(),
            slot: *slot,
            deletable: false,
        }
    } else {
        Op::CreateGlobalVar {
            name: name.to_string(),
            slot: *slot,
            deletable: false,
            is_lexical: false,
        }
    };
    ops.push(op);
}

/// Append a terminal `Return`, either the last expression value or `undefined`.
pub(crate) fn finish_program(
    mut ops: Vec<Op>,
    last_value: Option<u16>,
) -> Result<Vec<Op>, Vec<String>> {
    if let Some(register) = last_value {
        ops.push(Op::Return { src: register });
    } else {
        ops.push(Op::Const {
            dst: 0,
            value: Constant::Undefined,
        });
        ops.push(Op::Return { src: 0 });
    }
    Ok(ops)
}

/// Emit a constant `undefined` register and return it.
pub(crate) fn emit_undefined(ops: &mut Vec<Op>, next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: register,
        value: Constant::Undefined,
    });
    register
}

pub(crate) fn emit_move(ops: &mut Vec<Op>, dst: u16, src: u16) {
    if dst != src {
        ops.push(Op::Move { dst, src });
    }
}

/// Write `last` (or `undefined`) into `dst` so the statement has a completion.
pub(crate) fn seal_completion(ops: &mut Vec<Op>, dst: u16, last: Option<u16>) {
    match last {
        Some(src) => emit_move(ops, dst, src),
        None => ops.push(Op::Const {
            dst,
            value: Constant::Undefined,
        }),
    }
}
