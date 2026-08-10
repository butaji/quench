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
        if behavior == EvalBehavior::Global {
            crate::reduce_support::mirror_script_bindings(statement, locals, ops, next_register);
        }
    }
    Ok(())
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
