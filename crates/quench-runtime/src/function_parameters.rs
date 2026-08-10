use std::collections::HashMap;

use oxc::ast::ast::{BindingPattern, BindingPatternKind, FormalParameters};

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn bindings(
    formal: &FormalParameters<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    let count = u16::try_from(formal.items.len()).map_err(|_| too_many())?;
    let mut bindings = HashMap::new();
    let mut next = count.saturating_add(3);
    for (index, parameter) in formal.items.iter().enumerate() {
        let source = u16::try_from(index).map_err(|_| too_many())?;
        collect_bindings(&parameter.pattern, source, &mut next, &mut bindings)?;
    }
    if let Some(rest) = &formal.rest {
        let BindingPatternKind::BindingIdentifier(identifier) = &rest.argument.kind else {
            return Err(vec!["Unsupported rest parameter pattern".to_string()]);
        };
        bindings.insert(identifier.name.to_string(), count.saturating_add(2));
    }
    Ok((bindings, count))
}

pub(crate) fn is_simple(formal: &FormalParameters<'_>) -> bool {
    formal.rest.is_none()
        && formal
            .items
            .iter()
            .all(|item| matches!(item.pattern.kind, BindingPatternKind::BindingIdentifier(_)))
}

pub(crate) fn prefix(
    formal: &FormalParameters<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    captures: u16,
    implicit_arguments: bool,
) -> Option<Vec<Op>> {
    let barrier = eval_var_barrier(formal, implicit_arguments);
    let inherited = std::mem::replace(&mut facts.eval_var_barrier, barrier);
    let result = reduce_prefix(formal, facts, locals, captures);
    facts.eval_var_barrier = inherited;
    result
}

fn reduce_prefix(
    formal: &FormalParameters<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    captures: u16,
) -> Option<Vec<Op>> {
    let mut ops = Vec::new();
    let mut next = crate::reduce_support::register_base(locals);
    for (index, parameter) in formal.items.iter().enumerate() {
        if matches!(
            parameter.pattern.kind,
            BindingPatternKind::BindingIdentifier(_)
        ) {
            continue;
        }
        let slot = captures.saturating_add(u16::try_from(index).ok()?);
        let source = load_local(&mut ops, &mut next, slot);
        crate::binding_patterns::bind(
            &parameter.pattern,
            source,
            &mut ops,
            facts,
            &mut next,
            locals,
        )?;
    }
    Some(ops)
}

fn eval_var_barrier(formal: &FormalParameters<'_>, implicit_arguments: bool) -> Vec<String> {
    let mut names = formal
        .items
        .iter()
        .flat_map(|item| crate::binding_patterns::names(&item.pattern))
        .collect::<Vec<_>>();
    if let Some(rest) = &formal.rest {
        names.extend(crate::binding_patterns::names(&rest.argument));
    }
    if implicit_arguments {
        names.push("arguments".to_string());
    }
    names.sort_unstable();
    names.dedup();
    names
}

fn collect_bindings(
    pattern: &BindingPattern<'_>,
    direct_slot: u16,
    next: &mut u16,
    bindings: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    if let BindingPatternKind::BindingIdentifier(identifier) = &pattern.kind {
        bindings.insert(identifier.name.to_string(), direct_slot);
        return Ok(());
    }
    for name in crate::binding_patterns::names(pattern) {
        let slot = *next;
        *next = next.checked_add(1).ok_or_else(too_many)?;
        bindings.insert(name, slot);
    }
    Ok(())
}

fn load_local(ops: &mut Vec<Op>, next: &mut u16, slot: u16) -> u16 {
    let dst = take_register(next);
    ops.push(Op::LoadLocal { dst, slot });
    dst
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}

fn too_many() -> Vec<String> {
    vec!["Too many function parameters".to_string()]
}
