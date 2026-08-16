pub(crate) fn eval_bindings(
    program: &oxc::ast::ast::Program<'_>,
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
    strict: bool,
    global: bool,
) -> EvalBindings {
    let mut locals = bindings.iter().cloned().collect::<HashMap<String, u16>>();
    let mut next_slot = register_base(&locals);
    let collisions = crate::semantic_early::annex_b_lexical_collisions(program);
    let names = crate::semantic_early::var_declared_names(program)
        .into_iter()
        .filter(|name| {
            !collisions.contains(name) || bindings.iter().any(|(bound, _)| bound == name)
        })
        .collect::<Vec<_>>();
    let declared = if strict {
        shadow_names(&names, &mut locals, &mut next_slot);
        Vec::new()
    } else if global {
        reserve_names(&names, &mut locals, &mut next_slot)
    } else {
        shadow_eval_names(&names, reusable_var_names, &mut locals, &mut next_slot)
    };
    let behavior = if strict {
        EvalBehavior::Strict
    } else if global {
        EvalBehavior::Global
    } else {
        EvalBehavior::Local
    };
    let deletable = if !strict && !global {
        declared
    } else {
        Vec::new()
    };
    let mut prefix = eval_binding_prefix(&deletable);
    let lexical_names = crate::semantic_early::lexically_declared_names(program);
    let lexical = shadow_names(&lexical_names, &mut locals, &mut next_slot);
    prefix.extend(
        lexical
            .into_iter()
            .map(|(_, slot)| Op::MarkUninitialized { slot }),
    );
    (locals, next_slot, prefix, behavior, deletable)
}

fn shadow_eval_names(
    names: &[String],
    reusable: &[String],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) -> Vec<(String, u16)> {
    let names = names
        .iter()
        .filter(|name| !reusable.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    shadow_names(&names, locals, next_slot)
}

fn shadow_names(
    names: &[String],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) -> Vec<(String, u16)> {
    let mut names = names.to_vec();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(name.clone(), slot);
            (name, slot)
        })
        .collect()
}

fn eval_binding_prefix(declared: &[(String, u16)]) -> Vec<Op> {
    declared
        .iter()
        .map(|(name, slot)| Op::DeclareEvalBinding {
            name: name.clone(),
            slot: *slot,
        })
        .collect()
}

pub(crate) fn validate_eval_var_names(
    program: &oxc::ast::ast::Program<'_>,
    strict: bool,
    forbidden: &[String],
) -> Result<(), Vec<String>> {
    if strict {
        return Ok(());
    }
    let names = crate::semantic_early::var_declared_names(program);
    let conflict = names.iter().find(|name| forbidden.contains(name));
    match conflict {
        Some(name) => Err(vec![format!(
            "SyntaxError: eval var declaration conflicts with lexical binding `{name}`"
        )]),
        None => Ok(()),
    }
}

pub(crate) fn shadow_function_bindings(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    parameters: &HashMap<String, u16>,
) {
    for statement in statements {
        for name in declared_names(statement) {
            if !parameters.contains_key(&name) {
                locals.remove(&name);
            }
        }
    }
}

fn declared_names(statement: &oxc::ast::ast::Statement<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_declared_names(statement, &mut names);
    names
}

pub(crate) fn declared_names_in(statements: &[oxc::ast::ast::Statement<'_>]) -> Vec<String> {
    let mut names = Vec::new();
    collect_statement_names(statements, &mut names);
    names.sort_unstable();
    names.dedup();
    names
}

pub(crate) fn predeclare_lexicals(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        let oxc::ast::ast::Statement::VariableDeclaration(declaration) = statement else {
            continue;
        };
        if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var {
            continue;
        }
        for name in declaration
            .declarations
            .iter()
            .flat_map(|declarator| crate::binding_patterns::names(&declarator.id))
        {
            if locals.contains_key(&name) {
                continue;
            }
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(name.clone(), slot);
            locals.insert(format!("\0lexical-predeclared:{name}"), slot);
        }
    }
}

fn collect_declared_names(statement: &oxc::ast::ast::Statement<'_>, names: &mut Vec<String>) {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                names.push(identifier.name.to_string());
            }
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
            collect_declared_names(&statement.consequent, names);
            if let Some(alternate) = &statement.alternate {
                collect_declared_names(alternate, names);
            }
        }
        oxc::ast::ast::Statement::WhileStatement(statement) => {
            collect_declared_names(&statement.body, names);
        }
        oxc::ast::ast::Statement::DoWhileStatement(statement) => {
            collect_declared_names(&statement.body, names);
        }
        _ => {}
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
