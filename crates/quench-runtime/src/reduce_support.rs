//! Small reduction helpers shared across the reducer.

use std::collections::HashMap;

pub(crate) fn function_strictness(
    body: &oxc::ast::ast::FunctionBody<'_>,
    inherited: bool,
) -> crate::ops::FunctionStrictness {
    if inherited
        || body
            .directives
            .iter()
            .any(|directive| directive.directive.as_str() == "use strict")
    {
        crate::ops::FunctionStrictness::Strict
    } else {
        crate::ops::FunctionStrictness::Sloppy
    }
}

use crate::ops::{Constant, Op};

const SCRIPT_THIS_SLOT: &str = "\0script_this";

/// Highest allocated local slot plus one, used as the next register base.
pub(crate) fn register_base(locals: &HashMap<String, u16>) -> u16 {
    locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1))
}

/// Reserve function-scoped bindings before reducing executable statements.
pub(crate) fn predeclare_functions(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        for name in declared_names(statement) {
            reserve(&name, locals, next_slot);
        }
    }
}

pub(crate) fn reserve_names(
    names: &[String],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) -> Vec<(String, u16)> {
    let mut names = names.to_vec();
    names.sort_unstable();
    names.dedup();
    let mut declared = Vec::new();
    for name in names {
        if locals.contains_key(&name) {
            continue;
        }
        let slot = *next_slot;
        reserve(&name, locals, next_slot);
        declared.push((name, slot));
    }
    declared
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvalBehavior {
    Normal,
    Strict,
    Local,
    Global,
}

pub(crate) fn eval_bindings(
    program: &oxc::ast::ast::Program<'_>,
    bindings: &[(String, u16)],
    strict: bool,
    global: bool,
) -> (HashMap<String, u16>, u16, Vec<Op>, EvalBehavior) {
    let mut locals = bindings.iter().cloned().collect::<HashMap<_, _>>();
    let mut next_slot = register_base(&locals);
    let names = crate::semantic_early::var_declared_names(program);
    let declared = if strict {
        shadow_names(&names, &mut locals, &mut next_slot);
        Vec::new()
    } else {
        reserve_names(&names, &mut locals, &mut next_slot)
    };
    let behavior = if strict {
        EvalBehavior::Strict
    } else if global {
        EvalBehavior::Global
    } else {
        EvalBehavior::Local
    };
    let mut prefix = eval_binding_prefix(&declared, strict, global);
    let lexical_names = crate::semantic_early::lexically_declared_names(program);
    let lexical = shadow_names(&lexical_names, &mut locals, &mut next_slot);
    prefix.extend(
        lexical
            .into_iter()
            .map(|(_, slot)| Op::MarkUninitialized { slot }),
    );
    (locals, next_slot, prefix, behavior)
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

fn eval_binding_prefix(declared: &[(String, u16)], strict: bool, global: bool) -> Vec<Op> {
    if strict || global {
        return Vec::new();
    }
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
            collect_statement_names(&block.body, names);
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
    next_register: &mut u16,
) {
    match statement {
        oxc::ast::ast::Statement::FunctionDeclaration(function) => {
            if let Some(identifier) = &function.id {
                mirror_binding(identifier.name.as_str(), locals, ops, next_register);
            }
        }
        oxc::ast::ast::Statement::VariableDeclaration(declaration)
            if declaration.kind == oxc::ast::ast::VariableDeclarationKind::Var =>
        {
            for declarator in &declaration.declarations {
                if let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
                    &declarator.id.kind
                {
                    mirror_binding(identifier.name.as_str(), locals, ops, next_register);
                }
            }
        }
        _ => {}
    }
}

fn mirror_binding(
    name: &str,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) {
    let (Some(global_slot), Some(value_slot)) = (locals.get(SCRIPT_THIS_SLOT), locals.get(name))
    else {
        return;
    };
    let global = load_local(ops, next_register, *global_slot);
    let value = load_local(ops, next_register, *value_slot);
    ops.push(Op::SetProperty {
        object: global,
        key: name.to_string(),
        src: value,
    });
}

fn load_local(ops: &mut Vec<Op>, next_register: &mut u16, slot: u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal {
        dst: register,
        slot,
    });
    register
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
