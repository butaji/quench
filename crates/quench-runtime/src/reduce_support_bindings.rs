pub(crate) fn eval_bindings_without_program(
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
    strict: bool,
    global: bool,
) -> EvalBindings {
    finish_eval_bindings(bindings, reusable_var_names, &[], &[], strict, global)
}

pub(crate) fn eval_bindings(
    program: &oxc::ast::ast::Program<'_>,
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
    strict: bool,
    global: bool,
) -> EvalBindings {
    let collisions = crate::semantic_early::annex_b_lexical_collisions(program);
    let names = crate::semantic_early::var_declared_names(program)
        .into_iter()
        .filter(|name| {
            !collisions.contains(name) || bindings.iter().any(|(bound, _)| bound == name)
        })
        .collect::<Vec<_>>();
    let lexical_names = crate::semantic_early::lexically_declared_names(program);
    finish_eval_bindings(
        bindings,
        reusable_var_names,
        &names,
        &lexical_names,
        strict,
        global,
    )
}

fn finish_eval_bindings(
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
    names: &[String],
    lexical_names: &[String],
    strict: bool,
    global: bool,
) -> EvalBindings {
    let mut locals = bindings.iter().cloned().collect::<HashMap<String, u16>>();
    let mut next_slot = register_base(&locals);
    let declared = if strict {
        shadow_names(names, &mut locals, &mut next_slot);
        Vec::new()
    } else if global {
        reserve_names(names, &mut locals, &mut next_slot)
    } else {
        shadow_eval_names(names, reusable_var_names, &mut locals, &mut next_slot)
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
    let lexical = shadow_names(lexical_names, &mut locals, &mut next_slot);
    prefix.extend(
        lexical
            .into_iter()
            .map(|(_, slot)| Op::MarkUninitialized { slot, shared: true }),
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

pub(crate) fn annex_b_function_names(statements: &[oxc::ast::ast::Statement<'_>]) -> Vec<String> {
    statements
        .iter()
        .flat_map(annex_b_function_names_in)
        .collect()
}

fn annex_b_function_names_in(statement: &oxc::ast::ast::Statement<'_>) -> Vec<String> {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(function) => annex_b_plain_function_name(function)
            .into_iter()
            .collect(),
        oxc::ast::ast::Statement::BlockStatement(block) => annex_b_function_names(&block.body),
        oxc::ast::ast::Statement::IfStatement(statement) => {
            let mut names = annex_b_function_names(std::slice::from_ref(&statement.consequent));
            if let Some(alternate) = &statement.alternate {
                names.extend(annex_b_function_names(std::slice::from_ref(alternate)));
            }
            names
        }
        oxc::ast::ast::Statement::SwitchStatement(statement) => {
            annex_b_switch_function_names(statement)
        }
        oxc::ast::ast::Statement::LabeledStatement(statement) => {
            annex_b_function_names(std::slice::from_ref(&statement.body))
        }
        oxc::ast::ast::Statement::WhileStatement(statement) => {
            annex_b_function_names(std::slice::from_ref(&statement.body))
        }
        oxc::ast::ast::Statement::DoWhileStatement(statement) => {
            annex_b_function_names(std::slice::from_ref(&statement.body))
        }
        oxc::ast::ast::Statement::TryStatement(statement) => annex_b_try_function_names(statement),
        _ => Vec::new(),
    }
}

pub(crate) fn annex_b_plain_function_name(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<String> {
    if function.r#async || function.generator {
        return None;
    }
    function.id.as_ref().map(|identifier| identifier.name.to_string())
}

pub(crate) fn annex_b_switch_function_names(
    statement: &oxc::ast::ast::SwitchStatement<'_>,
) -> Vec<String> {
    statement
        .cases
        .iter()
        .flat_map(|case| annex_b_function_names(&case.consequent))
        .collect()
}

fn annex_b_try_function_names(statement: &oxc::ast::ast::TryStatement<'_>) -> Vec<String> {
    let mut names = annex_b_function_names(&statement.block.body);
    if let Some(handler) = &statement.handler {
        names.extend(annex_b_function_names(&handler.body.body));
    }
    if let Some(finalizer) = &statement.finalizer {
        names.extend(annex_b_function_names(&finalizer.body));
    }
    names
}

pub(crate) fn declared_names_in(statements: &[oxc::ast::ast::Statement<'_>]) -> Vec<String> {
    let mut names = Vec::new();
    collect_statement_names(statements, &mut names);
    names.sort_unstable();
    names.dedup();
    names
}

pub(crate) fn lexical_bound_names(statement: &oxc::ast::ast::Statement<'_>) -> Vec<String> {
    if let Some(declaration) = lexical_declaration(statement) {
        if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var {
            return Vec::new();
        }
        return declaration
            .declarations
            .iter()
            .flat_map(|declarator| crate::binding_patterns::names(&declarator.id))
            .collect();
    }
    class_bound_name(statement).into_iter().collect()
}

fn class_bound_name(statement: &oxc::ast::ast::Statement<'_>) -> Option<String> {
    match statement {
        oxc::ast::ast::Statement::ClassDeclaration(class) => {
            class.id.as_ref().map(|id| id.name.to_string())
        }
        oxc::ast::ast::Statement::ExportNamedDeclaration(export) => match &export.declaration {
            Some(oxc::ast::ast::Declaration::ClassDeclaration(class)) => {
                class.id.as_ref().map(|id| id.name.to_string())
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn lexical_declaration<'a>(
    statement: &'a oxc::ast::ast::Statement<'a>,
) -> Option<&'a oxc::ast::ast::VariableDeclaration<'a>> {
    match statement {
        oxc::ast::ast::Statement::VariableDeclaration(declaration) => Some(declaration),
        oxc::ast::ast::Statement::ExportNamedDeclaration(export) => {
            export.declaration.as_ref().and_then(|declaration| match declaration {
                oxc::ast::ast::Declaration::VariableDeclaration(declaration) => Some(&**declaration),
                _ => None,
            })
        }
        _ => None,
    }
}

pub(crate) fn predeclare_module_vars(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        for name in module_var_names(statement) {
            if locals.contains_key(&name) {
                continue;
            }
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(name, slot);
        }
    }
}

include!("reduce_support_bindings_rest.rs");
