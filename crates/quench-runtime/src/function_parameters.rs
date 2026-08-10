use std::collections::HashMap;

use oxc::ast::ast::{BindingPattern, BindingPatternKind, FormalParameters, PropertyKey};

use crate::{
    facts::ProgramDb,
    ops::{BinaryOp, Constant, Op},
};

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
        bind_pattern(
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

fn collect_bindings(
    pattern: &BindingPattern<'_>,
    direct_slot: u16,
    next: &mut u16,
    bindings: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => {
            bindings.insert(identifier.name.to_string(), direct_slot);
            Ok(())
        }
        BindingPatternKind::AssignmentPattern(pattern) => {
            collect_nested(&pattern.left, next, bindings)
        }
        BindingPatternKind::ArrayPattern(pattern) => {
            for element in pattern.elements.iter().flatten() {
                collect_nested(element, next, bindings)?;
            }
            collect_rest(pattern.rest.as_deref(), next, bindings)
        }
        BindingPatternKind::ObjectPattern(pattern) => {
            for property in &pattern.properties {
                collect_nested(&property.value, next, bindings)?;
            }
            collect_rest(pattern.rest.as_deref(), next, bindings)
        }
    }
}

fn collect_nested(
    pattern: &BindingPattern<'_>,
    next: &mut u16,
    bindings: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let slot = *next;
    *next = next.saturating_add(1);
    collect_bindings(pattern, slot, next, bindings)
}

fn collect_rest(
    rest: Option<&oxc::ast::ast::BindingRestElement<'_>>,
    next: &mut u16,
    bindings: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    if let Some(rest) = rest {
        collect_nested(&rest.argument, next, bindings)?;
    }
    Ok(())
}

fn bind_pattern(
    pattern: &BindingPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => {
            let slot = *locals.get(identifier.name.as_str())?;
            ops.push(Op::StoreLocal { slot, src: source });
            Some(())
        }
        BindingPatternKind::AssignmentPattern(pattern) => {
            let value = default_value(source, &pattern.right, ops, facts, next, locals)?;
            bind_pattern(&pattern.left, value, ops, facts, next, locals)
        }
        BindingPatternKind::ArrayPattern(pattern) => {
            bind_array(pattern, source, ops, facts, next, locals)
        }
        BindingPatternKind::ObjectPattern(pattern) => {
            bind_object(pattern, source, ops, facts, next, locals)
        }
    }
}

fn default_value(
    source: u16,
    fallback: &oxc::ast::ast::Expression<'_>,
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
    let mut consequent = Vec::new();
    let value = crate::reduce::reduce_expression(fallback, &mut consequent, facts, next, locals)?;
    consequent.push(Op::Return { src: value });
    let dst = take_register(next);
    ops.push(Op::Conditional {
        dst,
        condition,
        consequent,
        alternate: vec![Op::Return { src: source }],
    });
    Some(dst)
}

fn bind_array(
    pattern: &oxc::ast::ast::ArrayPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    for (index, element) in pattern.elements.iter().enumerate() {
        let Some(element) = element else { continue };
        let value = get_property(ops, next, source, index.to_string());
        bind_pattern(element, value, ops, facts, next, locals)?;
    }
    pattern.rest.is_none().then_some(())
}

fn bind_object(
    pattern: &oxc::ast::ast::ObjectPattern<'_>,
    source: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    for property in &pattern.properties {
        let key = property_key(&property.key)?;
        let value = get_property(ops, next, source, key);
        bind_pattern(&property.value, value, ops, facts, next, locals)?;
    }
    pattern.rest.is_none().then_some(())
}

fn property_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(key) => Some(key.name.to_string()),
        PropertyKey::StringLiteral(key) => Some(key.value.to_string()),
        PropertyKey::NumericLiteral(key) => Some(key.value.to_string()),
        _ => None,
    }
}

fn get_property(ops: &mut Vec<Op>, next: &mut u16, object: u16, key: String) -> u16 {
    let dst = take_register(next);
    ops.push(Op::GetProperty { dst, object, key });
    dst
}

fn load_local(ops: &mut Vec<Op>, next: &mut u16, slot: u16) -> u16 {
    let dst = take_register(next);
    ops.push(Op::LoadLocal { dst, slot });
    dst
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

fn too_many() -> Vec<String> {
    vec!["Too many function parameters".to_string()]
}
