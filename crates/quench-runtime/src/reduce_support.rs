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

pub(crate) fn has_strict_directive(program: &oxc::ast::ast::Program<'_>) -> bool {
    program
        .directives
        .iter()
        .any(|directive| directive.directive.as_str() == "use strict")
}

pub(crate) fn validate_parse(parsed: &oxc::parser::ParserReturn<'_>) -> Result<(), Vec<String>> {
    if parsed.panicked {
        return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
    }
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|error| format!("SyntaxError: {error}"))
            .collect());
    }
    validate_program(&parsed.program)
}

struct RegexpLiteralValidator<'a> {
    errors: Vec<String>,
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> oxc::ast::visit::Visit<'a> for RegexpLiteralValidator<'a> {
    fn visit_reg_exp_literal(&mut self, literal: &oxc::ast::ast::RegExpLiteral<'a>) {
        if let Some(raw) = literal.raw.as_ref() {
            let text = raw.as_str();
            if let Some(separator) = text.rfind('/') {
                let pattern = &text[1..separator];
                if let Err(error) = crate::regexp::validate_literal(pattern) {
                    self.errors.push(error);
                }
                if let Err(error) = crate::regexp::validate_pattern(pattern) {
                    self.errors.push(error);
                }
            }
        }
    }
}

pub(crate) fn validate_program(program: &oxc::ast::ast::Program<'_>) -> Result<(), Vec<String>> {
    let mut validator = RegexpLiteralValidator {
        errors: Vec::new(),
        marker: std::marker::PhantomData,
    };
    oxc::ast::visit::walk::walk_program(&mut validator, program);
    if validator.errors.is_empty() {
        Ok(())
    } else {
        Err(validator.errors)
    }
}

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

pub(crate) fn instantiate_script_declarations(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
    ops: &mut Vec<Op>,
    script: bool,
) {
    predeclare_functions(statements, locals, next_slot);
    if !script {
        return;
    }
    for statement in statements {
        for name in script_lexical_names(statement) {
            ops.push(Op::CheckGlobalVar {
                name: name.clone(),
                is_lexical: true,
            });
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            locals.insert(name.clone(), slot);
            ops.push(Op::MarkUninitialized { slot });
            ops.push(Op::DeclareEvalBinding { name, slot });
        }
    }
}

pub(crate) fn script_lexical_slot(
    statement: &oxc::ast::ast::Statement<'_>,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    script_lexical_names(statement)
        .first()
        .and_then(|name| locals.get(name).copied())
}

fn script_lexical_names(statement: &oxc::ast::ast::Statement<'_>) -> Vec<String> {
    match statement {
        oxc::ast::ast::Statement::VariableDeclaration(declaration)
            if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var =>
        {
            declaration
                .declarations
                .iter()
                .flat_map(|declarator| crate::binding_patterns::names(&declarator.id))
                .collect()
        }
        oxc::ast::ast::Statement::ClassDeclaration(class) => class
            .id
            .iter()
            .map(|identifier| identifier.name.to_string())
            .collect(),
        _ => Vec::new(),
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

pub(crate) type EvalBindings = (
    HashMap<String, u16>,
    u16,
    Vec<Op>,
    EvalBehavior,
    Vec<(String, u16)>,
);

pub(crate) fn eval_bindings(
    program: &oxc::ast::ast::Program<'_>,
    bindings: &[(String, u16)],
    strict: bool,
    global: bool,
) -> EvalBindings {
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
