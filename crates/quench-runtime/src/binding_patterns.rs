use std::collections::HashMap;

use oxc::ast::ast::{BindingPattern, BindingPatternKind, Expression, PropertyKey};

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
            let value = default_value(source, &pattern.right, ops, facts, next, locals)?;
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

fn default_value(
    source: u16,
    fallback: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let undefined = emit_const(ops, next, Constant::Undefined);
    let condition = take_register(next);
    ops.push(Op::Binary {
        dst: condition,
        operator: BinaryOp::StrictEqual,
        lhs: source,
        rhs: undefined,
    });
    let consequent = fallback_ops(fallback, facts, next, locals)?;
    let dst = take_register(next);
    ops.push(Op::Conditional {
        dst,
        condition,
        consequent,
        alternate: vec![Op::Return { src: source }],
    });
    Some(dst)
}

fn fallback_ops(
    fallback: &Expression<'_>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Vec<Op>> {
    let mut ops = Vec::new();
    let value = crate::reduce::reduce_expression(fallback, &mut ops, facts, next, locals)?;
    ops.push(Op::Return { src: value });
    Some(ops)
}

fn bind_array(
    pattern: &oxc::ast::ast::ArrayPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let iterator = take_register(next);
    ops.push(Op::GetIterator {
        dst: iterator,
        iterable: source,
    });
    for element in &pattern.elements {
        let value = take_register(next);
        ops.push(Op::IteratorStep {
            dst: value,
            iterator,
        });
        if let Some(element) = element {
            bind(element, value, ops, facts, next, locals)?;
        }
    }
    bind_array_rest(pattern.rest.as_deref(), iterator, ops, facts, next, locals)
}

fn bind_array_rest(
    rest: Option<&oxc::ast::ast::BindingRestElement<'_>>,
    iterator: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let Some(rest) = rest else { return Some(()) };
    let value = take_register(next);
    ops.push(Op::IteratorRest {
        dst: value,
        iterator,
    });
    bind(&rest.argument, value, ops, facts, next, locals)
}

fn bind_object(
    pattern: &oxc::ast::ast::ObjectPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    ops.push(Op::RequireObjectCoercible { src: source });
    for property in &pattern.properties {
        let key = reduce_key(&property.key, ops, facts, next, locals)?;
        let resolved = pre_resolve(&property.value, ops, next, locals)?;
        let value = get_property(key, source, ops, next);
        match resolved {
            Some(resolved) => {
                bind_resolved(&property.value, value, resolved, ops, facts, next, locals)?
            }
            None => bind(&property.value, value, ops, facts, next, locals)?,
        }
    }
    pattern.rest.is_none().then_some(())
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
        BindingPatternKind::AssignmentPattern(pattern) => {
            default_value(source, &pattern.right, ops, facts, next, locals)?
        }
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

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(key) => Some(key.name.to_string()),
        PropertyKey::StringLiteral(key) => Some(key.value.to_string()),
        PropertyKey::NumericLiteral(key) => Some(key.value.to_string()),
        _ => None,
    }
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
