use std::collections::HashMap;

use oxc::ast::ast::{
    AssignmentTarget, AssignmentTargetMaybeDefault, AssignmentTargetProperty, BindingPattern,
    BindingPatternKind, Expression, PropertyKey,
};

use crate::{
    facts::ProgramDb,
    ops::{BinaryOp, Constant, Op},
};

struct ResolvedBinding {
    target: u16,
    name: String,
    slot: u16,
}

enum ReducedKey {
    Static(String),
    Dynamic(u16),
}

pub(crate) fn names(pattern: &BindingPattern<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_names(pattern, &mut names);
    names
}

pub(crate) fn bind(
    pattern: &BindingPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => {
            store_identifier(identifier.name.as_str(), source, ops, next, locals)
        }
        BindingPatternKind::AssignmentPattern(pattern) => {
            let name = direct_name(&pattern.left);
            let value = default_value(source, &pattern.right, name, ops, facts, next, locals)?;
            bind(&pattern.left, value, ops, facts, next, locals)
        }
        BindingPatternKind::ArrayPattern(pattern) => {
            bind_array(pattern, source, ops, facts, next, locals)
        }
        BindingPatternKind::ObjectPattern(pattern) => {
            bind_object(pattern, source, ops, facts, next, locals)
        }
    }
}
pub(crate) fn assign_target(
    target: &AssignmentTarget<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    match target {
        AssignmentTarget::ArrayAssignmentTarget(pattern) => {
            assign_array(pattern, source, ops, facts, next, locals)
        }
        AssignmentTarget::ObjectAssignmentTarget(pattern) => {
            assign_object(pattern, source, ops, facts, next, locals)
        }
        _ => crate::reduce::reduce_assignments::put_assignment_target(
            target, source, ops, facts, next, locals,
        ),
    }
}

fn assign_maybe_default(
    target: &AssignmentTargetMaybeDefault<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let (target, value) = match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => (
            &default.binding,
            default_value(
                source,
                &default.init,
                assignment_name(&default.binding),
                ops,
                facts,
                next,
                locals,
            )?,
        ),
        _ => (target.as_assignment_target()?, source),
    };
    assign_target(target, value, ops, facts, next, locals)
}

fn assign_array(
    pattern: &oxc::ast::ast::ArrayAssignmentTarget<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let iterator = iterator_start(source, ops, next);
    let mut body = Vec::new();
    for element in &pattern.elements {
        let place = element.as_ref().and_then(|element| {
            crate::reduce::reduce_assignments::maybe_default_place(
                element, &mut body, facts, next, locals,
            )
        });
        let value = iterator_step(iterator, &mut body, next);
        if let Some(element) = element {
            assign_prepared(element, place, value, &mut body, facts, next, locals)?;
        }
    }
    if let Some(rest) = &pattern.rest {
        assign_rest_target(&rest.target, iterator, &mut body, facts, next, locals)?;
    }
    ops.push(Op::IteratorBinding {
        iterator,
        body,
        close_normal: true,
    });
    Some(())
}

include!("binding_pattern_rest.rs");

fn assign_prepared(
    target: &AssignmentTargetMaybeDefault<'_>,
    place: Option<crate::reduce::reduce_assignments::Place>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let Some(place) = place else {
        return assign_maybe_default(target, source, ops, facts, next, locals);
    };
    let value = match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => default_value(
            source,
            &default.init,
            assignment_name(&default.binding),
            ops,
            facts,
            next,
            locals,
        )?,
        _ => source,
    };
    crate::reduce::reduce_assignments::put(place, value, ops)
}

fn assign_object(
    pattern: &oxc::ast::ast::ObjectAssignmentTarget<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    ops.push(Op::RequireObjectCoercible { src: source });
    let mut excluded = Vec::with_capacity(pattern.properties.len());
    for property in &pattern.properties {
        excluded.push(assign_property(property, source, ops, facts, next, locals)?);
    }
    let Some(rest) = &pattern.rest else {
        return Some(());
    };
    let target = crate::reduce::reduce_assignments::emit_object_rest(source, excluded, ops, next);
    assign_target(&rest.target, target, ops, facts, next, locals)
}

fn assign_property(
    property: &AssignmentTargetProperty<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    match property {
        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(property) => {
            assign_identifier_property(property, source, ops, facts, next, locals)
        }
        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
            assign_named_property(property, source, ops, facts, next, locals)
        }
    }
}

fn assign_identifier_property(
    property: &oxc::ast::ast::AssignmentTargetPropertyIdentifier<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let name = property.binding.name.as_str();
    let place =
        crate::reduce::reduce_assignments::identifier_assignment_place(name, facts.strict, locals);
    let excluded = emit_const(ops, next, Constant::String(name.to_string()));
    let value = get_property(ReducedKey::Static(name.to_string()), source, ops, next);
    let value = match &property.init {
        Some(init) => default_value(value, init, Some(name), ops, facts, next, locals)?,
        None => value,
    };
    crate::reduce::reduce_assignments::put(place, value, ops)?;
    Some(excluded)
}

fn assign_named_property(
    property: &oxc::ast::ast::AssignmentTargetPropertyProperty<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let key = reduce_key(&property.name, ops, facts, next, locals)?;
    let excluded = reduced_key_register(&key, ops, next);
    let place = crate::reduce::reduce_assignments::maybe_default_place(
        &property.binding,
        ops,
        facts,
        next,
        locals,
    );
    let value = get_property(key, source, ops, next);
    assign_property_value(property, (place, value), ops, facts, next, locals)?;
    Some(excluded)
}

fn assign_property_value(
    property: &oxc::ast::ast::AssignmentTargetPropertyProperty<'_>,
    resolved: (Option<crate::reduce::reduce_assignments::Place>, u16),
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let (place, value) = resolved;
    match place {
        Some(place) => {
            let value = assignment_default(&property.binding, value, ops, facts, next, locals)?;
            crate::reduce::reduce_assignments::put(place, value, ops)
        }
        None => assign_maybe_default(&property.binding, value, ops, facts, next, locals),
    }
}

fn assignment_default(
    target: &AssignmentTargetMaybeDefault<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => default_value(
            source,
            &default.init,
            assignment_name(&default.binding),
            ops,
            facts,
            next,
            locals,
        ),
        _ => Some(source),
    }
}

fn collect_names(pattern: &BindingPattern<'_>, names: &mut Vec<String>) {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => {
            names.push(identifier.name.to_string());
        }
        BindingPatternKind::AssignmentPattern(pattern) => collect_names(&pattern.left, names),
        BindingPatternKind::ArrayPattern(pattern) => {
            for element in pattern.elements.iter().flatten() {
                collect_names(element, names);
            }
            collect_rest(pattern.rest.as_deref(), names);
        }
        BindingPatternKind::ObjectPattern(pattern) => {
            for property in &pattern.properties {
                collect_names(&property.value, names);
            }
            collect_rest(pattern.rest.as_deref(), names);
        }
    }
}

fn collect_rest(rest: Option<&oxc::ast::ast::BindingRestElement<'_>>, names: &mut Vec<String>) {
    if let Some(rest) = rest {
        collect_names(&rest.argument, names);
    }
}

fn store_identifier(
    name: &str,
    source: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let slot = *locals.get(name)?;
    let target = take_register(next);
    ops.push(Op::ResolveBindingTarget {
        dst: target,
        name: name.to_string(),
    });
    ops.push(Op::InitializeResolvedBinding {
        target,
        slot,
        name: name.to_string(),
        src: source,
    });
    Some(())
}

include!("binding_pattern_names.rs");

fn bind_array(
    pattern: &oxc::ast::ast::ArrayPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let iterator = iterator_start(source, ops, next);
    let mut body = Vec::new();
    for element in &pattern.elements {
        let value = iterator_step(iterator, &mut body, next);
        if let Some(element) = element {
            bind(element, value, &mut body, facts, next, locals)?;
        }
    }
    if let Some(rest) = pattern.rest.as_deref() {
        let value = iterator_rest(iterator, &mut body, next);
        bind(&rest.argument, value, &mut body, facts, next, locals)?;
    }
    ops.push(Op::IteratorBinding {
        iterator,
        body,
        close_normal: false,
    });
    Some(())
}

include!("binding_pattern_iterator_ops.rs");

fn bind_object(
    pattern: &oxc::ast::ast::ObjectPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    ops.push(Op::RequireObjectCoercible { src: source });
    let mut excluded = Vec::with_capacity(pattern.properties.len());
    for property in &pattern.properties {
        let key = reduce_key(&property.key, ops, facts, next, locals)?;
        excluded.push(reduced_key_register(&key, ops, next));
        let resolved = pre_resolve(&property.value, ops, next, locals)?;
        let value = get_property(key, source, ops, next);
        match resolved {
            Some(resolved) => {
                bind_resolved(&property.value, value, resolved, ops, facts, next, locals)?
            }
            None => bind(&property.value, value, ops, facts, next, locals)?,
        }
    }
    let Some(rest) = pattern.rest.as_deref() else {
        return Some(());
    };
    let value = crate::reduce::reduce_assignments::emit_object_rest(source, excluded, ops, next);
    bind(&rest.argument, value, ops, facts, next, locals)
}

fn pre_resolve(
    pattern: &BindingPattern<'_>,
    ops: &mut Vec<Op>,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Option<ResolvedBinding>> {
    let Some(name) = direct_name(pattern) else {
        return Some(None);
    };
    let slot = *locals.get(name)?;
    let target = take_register(next);
    ops.push(Op::ResolveBindingTarget {
        dst: target,
        name: name.to_string(),
    });
    Some(Some(ResolvedBinding {
        target,
        name: name.to_string(),
        slot,
    }))
}

fn direct_name<'a>(pattern: &'a BindingPattern<'_>) -> Option<&'a str> {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => Some(identifier.name.as_str()),
        BindingPatternKind::AssignmentPattern(pattern) => direct_name(&pattern.left),
        _ => None,
    }
}

fn bind_resolved(
    pattern: &BindingPattern<'_>,
    source: u16,
    resolved: ResolvedBinding,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let value = match &pattern.kind {
        BindingPatternKind::AssignmentPattern(pattern) => default_value(
            source,
            &pattern.right,
            direct_name(&pattern.left),
            ops,
            facts,
            next,
            locals,
        )?,
        BindingPatternKind::BindingIdentifier(_) => source,
        _ => return None,
    };
    ops.push(Op::InitializeResolvedBinding {
        target: resolved.target,
        slot: resolved.slot,
        name: resolved.name,
        src: value,
    });
    Some(())
}

fn reduce_key(
    key: &PropertyKey<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<ReducedKey> {
    if let Some(key) = static_key(key) {
        return Some(ReducedKey::Static(key));
    }
    let src = crate::reduce::reduce_expression(key.as_expression()?, ops, facts, next, locals)?;
    let key = take_register(next);
    ops.push(Op::ToPropertyKey { dst: key, src });
    Some(ReducedKey::Dynamic(key))
}

fn get_property(key: ReducedKey, object: u16, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let dst = take_register(next);
    match key {
        ReducedKey::Static(key) => ops.push(Op::GetProperty { dst, object, key }),
        ReducedKey::Dynamic(key) => ops.push(Op::GetPropertyDynamic { dst, object, key }),
    }
    dst
}

fn reduced_key_register(key: &ReducedKey, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    match key {
        ReducedKey::Static(key) => emit_const(ops, next, Constant::String(key.clone())),
        ReducedKey::Dynamic(key) => *key,
    }
}

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    if let PropertyKey::BigIntLiteral(value) = key {
        return crate::literal::bigint_value(value);
    }
    key.static_name().map(|name| name.into_owned())
}

fn emit_const(ops: &mut Vec<Op>, next: &mut u16, value: Constant) -> u16 {
    let dst = take_register(next);
    ops.push(Op::Const { dst, value });
    dst
}
fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}
