use std::collections::{HashMap, HashSet};

use oxc::ast::ast::Statement;

use crate::{facts::ProgramDb, ops::Op, reduce_support::EvalBehavior};

pub(super) fn directive_completion(
    program: &oxc::ast::ast::Program<'_>,
    inherited_strict: bool,
) -> Option<String> {
    program
        .directives
        .get(usize::from(inherited_strict)..)?
        .last()
        .map(|directive| directive.directive.to_string())
}

pub(super) fn emit_directive(
    ops: &mut Vec<Op>,
    next: &mut u16,
    value: Option<String>,
) -> Option<u16> {
    let value = value?;
    let register = *next;
    *next = next.saturating_add(1);
    ops.push(Op::Const {
        dst: register,
        value: crate::ops::Constant::String(value),
    });
    Some(register)
}

type ReductionState<'a> = (
    &'a mut Vec<Op>,
    &'a mut u16,
    &'a mut u16,
    &'a mut HashMap<String, u16>,
);

pub(super) fn instantiate_functions(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    state: ReductionState<'_>,
    behavior: EvalBehavior,
) -> Result<(), Vec<String>> {
    let (ops, next_register, next_slot, locals) = state;
    if !facts.strict && is_global_behavior(behavior) {
        reserve_annex_b_bindings(statements, locals, next_slot);
    }
    let (selected, variables, lexical) = instantiate_context(statements, behavior, !facts.strict);
    if is_global_behavior(behavior) {
        emit_global_checks(
            &selected,
            &variables,
            &lexical,
            ops,
            behavior == EvalBehavior::Global,
        );
    }
    emit_function_declarations(
        statements,
        facts,
        ops,
        next_register,
        next_slot,
        locals,
        behavior,
    )?;
    if is_global_behavior(behavior) {
        emit_global_bindings_and_checks(
            &variables,
            &lexical,
            locals,
            ops,
            behavior == EvalBehavior::Global,
        )?;
    }
    if matches!(behavior, EvalBehavior::Script) {
        emit_global_lexical_bindings(statements, locals, ops);
    }
    Ok(())
}

fn instantiate_context<'a>(
    statements: &'a [Statement<'a>],
    behavior: EvalBehavior,
    annex_b: bool,
) -> (Vec<&'a Statement<'a>>, Vec<String>, Vec<String>) {
    let selected = selected_functions(statements);
    let winners = selected_function_names(&selected);
    let mut variables = global_variable_names(statements, &winners);
    if annex_b {
        variables.extend(crate::reduce_support::annex_b_function_names(statements));
        variables.sort_unstable();
        variables.dedup();
    }
    let lexical = if matches!(behavior, EvalBehavior::Script) {
        lexical_variable_names(statements)
    } else {
        Vec::new()
    };
    (selected, variables, lexical)
}

fn emit_function_declarations(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    behavior: EvalBehavior,
) -> Result<(), Vec<String>> {
    let emit_global = is_global_behavior(behavior);
    for statement in selected_functions(statements) {
        let Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        super::reduce_statements::reduce_function_declaration(
            function,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )?;
        if emit_global {
            emit_global_function(function, locals, ops, behavior == EvalBehavior::Global);
        }
    }
    Ok(())
}

fn emit_global_bindings_and_checks(
    variables: &[String],
    lexical: &[String],
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    deletable: bool,
) -> Result<(), Vec<String>> {
    emit_global_vars(variables, lexical, locals, ops, deletable);
    Ok(())
}

fn is_global_behavior(behavior: EvalBehavior) -> bool {
    matches!(behavior, EvalBehavior::Global | EvalBehavior::Script)
}

pub(super) fn emit_global_lexical_bindings(
    statements: &[Statement<'_>],
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
) {
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration)
                if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var =>
            {
                let immutable = declaration.kind == oxc::ast::ast::VariableDeclarationKind::Const;
                emit_lexical_declaration(declaration, locals, ops, immutable);
            }
            Statement::ClassDeclaration(class) => emit_class_binding(class, locals, ops),
            _ => {}
        }
    }
}

fn emit_lexical_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    immutable: bool,
) {
    for declarator in &declaration.declarations {
        for name in crate::binding_patterns::names(&declarator.id) {
            emit_lexical_binding(name, locals, ops, immutable);
        }
    }
}

fn emit_class_binding(
    class: &oxc::ast::ast::Class<'_>,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
) {
    if let Some(identifier) = &class.id {
        emit_lexical_binding(identifier.name.to_string(), locals, ops, false);
    }
}

fn emit_lexical_binding(
    name: String,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    immutable: bool,
) {
    if let Some(slot) = locals.get(&name) {
        ops.push(Op::DeclareGlobalLexicalBinding {
            name,
            slot: *slot,
            immutable,
        });
    }
}

fn selected_function_names(statements: &[&Statement<'_>]) -> HashSet<String> {
    statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::FunctionDeclaration(function) => {
                function.id.as_ref().map(|id| id.name.to_string())
            }
            _ => None,
        })
        .collect()
}

fn global_variable_names(statements: &[Statement<'_>], winners: &HashSet<String>) -> Vec<String> {
    crate::reduce_support::declared_names_in(statements)
        .into_iter()
        .filter(|name| !winners.contains(name))
        .collect()
}

fn reserve_annex_b_bindings(
    statements: &[Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => {
                reserve_annex_b_bindings(&block.body, locals, next_slot);
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id {
                    reserve_annex_b_name(identifier.name.as_str(), locals, next_slot);
                }
            }
            Statement::IfStatement(statement) => {
                reserve_annex_b_bindings(
                    std::slice::from_ref(&statement.consequent),
                    locals,
                    next_slot,
                );
                if let Some(alternate) = &statement.alternate {
                    reserve_annex_b_bindings(std::slice::from_ref(alternate), locals, next_slot);
                }
            }
            _ => {}
        }
    }
}

fn reserve_annex_b_name(name: &str, locals: &mut HashMap<String, u16>, next_slot: &mut u16) {
    if !locals.contains_key(name) {
        locals.insert(name.to_string(), *next_slot);
        *next_slot = next_slot.saturating_add(1);
    }
}

fn lexical_variable_names(statements: &[Statement<'_>]) -> Vec<String> {
    let mut names = Vec::new();
    for statement in statements {
        if let Statement::VariableDeclaration(declaration) = statement {
            if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var {
                for declarator in &declaration.declarations {
                    names.extend(crate::binding_patterns::names(&declarator.id));
                }
            }
        } else if let Statement::ClassDeclaration(class) = statement {
            if let Some(id) = &class.id {
                names.push(id.name.to_string());
            }
        }
    }
    names
}

fn emit_global_checks(
    functions: &[&Statement<'_>],
    variables: &[String],
    lexical: &[String],
    ops: &mut Vec<Op>,
    is_eval: bool,
) {
    for statement in functions.iter().rev() {
        let Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        if let Some(identifier) = &function.id {
            ops.push(Op::CheckGlobalFunction {
                name: identifier.name.to_string(),
            });
        }
    }
    let lexical_set: std::collections::HashSet<&String> = lexical.iter().collect();
    let seen: std::collections::HashSet<&String> = variables.iter().collect();
    for name in variables {
        ops.push(Op::CheckGlobalVar {
            name: name.clone(),
            is_lexical: lexical_set.contains(name),
            is_eval,
        });
    }
    for name in lexical {
        if !seen.contains(name) {
            ops.push(Op::CheckGlobalVar {
                name: name.clone(),
                is_lexical: true,
                is_eval,
            });
        }
    }
}

fn emit_global_function(
    function: &oxc::ast::ast::Function<'_>,
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    deletable: bool,
) {
    let Some(identifier) = &function.id else {
        return;
    };
    let Some(slot) = locals.get(identifier.name.as_str()) else {
        return;
    };
    ops.push(Op::CreateGlobalFunction {
        name: identifier.name.to_string(),
        slot: *slot,
        deletable,
    });
}

fn emit_global_vars(
    names: &[String],
    lexical: &[String],
    locals: &HashMap<String, u16>,
    ops: &mut Vec<Op>,
    deletable: bool,
) {
    let lexical_set: std::collections::HashSet<&String> = lexical.iter().collect();
    let seen: std::collections::HashSet<&String> = names.iter().collect();
    for name in names {
        if let Some(slot) = locals.get(name) {
            ops.push(Op::CreateGlobalVar {
                name: name.clone(),
                slot: *slot,
                deletable,
                is_lexical: lexical_set.contains(name),
            });
        }
    }
    for name in lexical {
        if !seen.contains(name) {
            if let Some(slot) = locals.get(name) {
                ops.push(Op::CreateGlobalVar {
                    name: name.clone(),
                    slot: *slot,
                    deletable,
                    is_lexical: true,
                });
            }
        }
    }
}

fn selected_functions<'a>(statements: &'a [Statement<'a>]) -> Vec<&'a Statement<'a>> {
    let mut names = HashSet::new();
    let mut selected = statements
        .iter()
        .rev()
        .filter(|statement| select_function(statement, &mut names))
        .collect::<Vec<_>>();
    selected.reverse();
    selected
}

fn select_function(statement: &Statement<'_>, names: &mut HashSet<String>) -> bool {
    let Statement::FunctionDeclaration(function) = statement else {
        return false;
    };
    function
        .id
        .as_ref()
        .is_some_and(|identifier| names.insert(identifier.name.to_string()))
}
