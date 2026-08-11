use std::collections::HashMap;

use oxc::ast::ast::{AssignmentOperator, AssignmentTarget, Expression};

use crate::{facts::ProgramDb, literal::reduce_operator, ops::Op};

#[derive(Clone)]
pub(crate) enum PlaceKey {
    Static(String),
    Dynamic(u16),
}

#[derive(Clone)]
pub(crate) enum Place {
    Local {
        slot: u16,
    },
    Name {
        name: String,
        strict: bool,
    },
    Property {
        object: u16,
        key: PlaceKey,
        strict: bool,
    },
    Private {
        object: u16,
        name: crate::facts::PrivateNameId,
    },
    Super {
        key: PlaceKey,
    },
}

pub fn reduce_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if matches!(
        assignment.left,
        AssignmentTarget::ArrayAssignmentTarget(_) | AssignmentTarget::ObjectAssignmentTarget(_)
    ) {
        let value = crate::reduce::reduce_expression(&assignment.right, ops, facts, next, locals)?;
        crate::binding_patterns::assign_target(&assignment.left, value, ops, facts, next, locals)?;
        return Some(value);
    }
    let mut place = reduce_place(&assignment.left, ops, facts, next, locals)?;
    if let Place::Name { name, strict: true } = &place {
        ops.push(Op::CheckStrictName { key: name.clone() });
    }
    if assignment.operator.is_logical() {
        return crate::logical::reduce_assignment(assignment, place, ops, facts, next, locals);
    }
    let lhs = if assignment.operator == AssignmentOperator::Assign {
        None
    } else {
        prepare_get(&mut place, ops, next);
        Some(get(&place, ops, next)?)
    };
    let rhs = crate::reduce::reduce_expression(&assignment.right, ops, facts, next, locals)?;
    infer_assignment_name(assignment, rhs, ops);
    let value = assignment_value(assignment.operator, lhs, rhs, ops, next)?;
    put(place, value, ops)?;
    Some(value)
}

fn infer_assignment_name(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    value: u16,
    ops: &mut Vec<Op>,
) {
    if assignment.operator != AssignmentOperator::Assign
        || !crate::binding_patterns::anonymous_function_definition(&assignment.right)
    {
        return;
    }
    let AssignmentTarget::AssignmentTargetIdentifier(identifier) = &assignment.left else {
        return;
    };
    ops.push(Op::SetFunctionName {
        function: value,
        name: identifier.name.to_string(),
    });
}

pub(crate) fn put_assignment_target(
    target: &AssignmentTarget<'_>,
    value: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let place = reduce_place(target, ops, facts, next, locals)?;
    put(place, value, ops)
}

pub(crate) fn reduce_place(
    target: &AssignmentTarget<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Place> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(identifier) => Some(identifier_place(
            identifier.name.as_str(),
            facts.strict,
            locals,
        )),
        AssignmentTarget::StaticMemberExpression(member) => member_place(
            &member.object,
            PlaceKey::Static(member.property.name.to_string()),
            ops,
            facts,
            next,
            locals,
        ),
        AssignmentTarget::PrivateFieldExpression(member) => private_place(
            &member.object,
            facts.private_name(member.field.span)?,
            ops,
            facts,
            next,
            locals,
        ),
        AssignmentTarget::ComputedMemberExpression(member) => {
            computed_place(member, ops, facts, next, locals)
        }
        _ => None,
    }
}

pub(crate) fn maybe_default_place(
    target: &oxc::ast::ast::AssignmentTargetMaybeDefault<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Place> {
    let target = match target {
        oxc::ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
            &default.binding
        }
        _ => target.as_assignment_target()?,
    };
    if matches!(
        target,
        AssignmentTarget::ArrayAssignmentTarget(_) | AssignmentTarget::ObjectAssignmentTarget(_)
    ) {
        return None;
    }
    reduce_place(target, ops, facts, next, locals)
}

pub(crate) fn emit_object_rest(
    source: u16,
    excluded: Vec<u16>,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> u16 {
    let target = take_register(next);
    ops.push(Op::MakeObject {
        dst: target,
        properties: Vec::new(),
    });
    ops.push(Op::CopyDataProperties {
        target,
        source,
        excluded,
    });
    target
}

pub(crate) fn identifier_assignment_place(
    name: &str,
    strict: bool,
    locals: &HashMap<String, u16>,
) -> Place {
    identifier_place(name, strict, locals)
}

pub(crate) fn reduce_simple_place(
    target: &oxc::ast::ast::SimpleAssignmentTarget<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Place> {
    match target {
        oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => Some(
            identifier_place(identifier.name.as_str(), facts.strict, locals),
        ),
        oxc::ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member) => member_place(
            &member.object,
            PlaceKey::Static(member.property.name.to_string()),
            ops,
            facts,
            next,
            locals,
        ),
        oxc::ast::ast::SimpleAssignmentTarget::PrivateFieldExpression(member) => private_place(
            &member.object,
            facts.private_name(member.field.span)?,
            ops,
            facts,
            next,
            locals,
        ),
        oxc::ast::ast::SimpleAssignmentTarget::ComputedMemberExpression(member) => {
            computed_place(member, ops, facts, next, locals)
        }
        _ => None,
    }
}

fn identifier_place(name: &str, strict: bool, locals: &HashMap<String, u16>) -> Place {
    locals.get(name).map_or_else(
        || Place::Name {
            name: name.to_string(),
            strict,
        },
        |slot| Place::Local { slot: *slot },
    )
}

fn computed_place(
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Place> {
    let object = reduce_member_object(&member.object, ops, facts, next, locals)?;
    let key = crate::reduce::reduce_expression(&member.expression, ops, facts, next, locals)?;
    finish_member_place(object, PlaceKey::Dynamic(key), facts.strict)
}

pub(crate) fn prepare_get(place: &mut Place, ops: &mut Vec<Op>, next: &mut u16) {
    let (object, key) = match place {
        Place::Property {
            object,
            key: PlaceKey::Dynamic(key),
            ..
        } => (Some(*object), key),
        Place::Super {
            key: PlaceKey::Dynamic(key),
        } => (None, key),
        _ => return,
    };
    if let Some(object) = object {
        ops.push(Op::RequireObjectCoercible { src: object });
    }
    let dst = take_register(next);
    ops.push(Op::ToPropertyKey { dst, src: *key });
    *key = dst;
}

fn member_place(
    expression: &Expression<'_>,
    key: PlaceKey,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Place> {
    let object = reduce_member_object(expression, ops, facts, next, locals)?;
    finish_member_place(object, key, facts.strict)
}

fn private_place(
    expression: &Expression<'_>,
    name: crate::facts::PrivateNameId,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Place> {
    let object = crate::reduce::reduce_expression(expression, ops, facts, next, locals)?;
    Some(Place::Private { object, name })
}

fn reduce_member_object(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Option<u16>> {
    if matches!(expression, Expression::Super(_)) {
        return Some(None);
    }
    crate::reduce::reduce_expression(expression, ops, facts, next, locals).map(Some)
}

fn finish_member_place(object: Option<u16>, key: PlaceKey, strict: bool) -> Option<Place> {
    match object {
        Some(object) => Some(Place::Property {
            object,
            key,
            strict,
        }),
        None => Some(Place::Super { key }),
    }
}

pub(crate) fn get(place: &Place, ops: &mut Vec<Op>, next: &mut u16) -> Option<u16> {
    let dst = take_register(next);
    match place {
        Place::Local { slot } => ops.push(Op::LoadLocal { dst, slot: *slot }),
        Place::Name { name, .. } => ops.push(Op::ResolveName {
            dst,
            key: name.clone(),
        }),
        Place::Property { object, key, .. } => emit_property_get(ops, dst, *object, key),
        Place::Private { object, name } => ops.push(Op::GetPrivate {
            dst,
            object: *object,
            name: *name,
        }),
        Place::Super {
            key: PlaceKey::Static(key),
        } => ops.push(Op::GetSuperProperty {
            dst,
            key: key.clone(),
        }),
        Place::Super {
            key: PlaceKey::Dynamic(key),
        } => ops.push(Op::GetSuperPropertyDynamic { dst, key: *key }),
    }
    Some(dst)
}

fn emit_property_get(ops: &mut Vec<Op>, dst: u16, object: u16, key: &PlaceKey) {
    match key {
        PlaceKey::Static(key) => ops.push(Op::GetProperty {
            dst,
            object,
            key: key.clone(),
        }),
        PlaceKey::Dynamic(key) => ops.push(Op::GetPropertyDynamic {
            dst,
            object,
            key: *key,
        }),
    }
}

pub(crate) fn put(place: Place, value: u16, ops: &mut Vec<Op>) -> Option<()> {
    match place {
        Place::Local { slot } => {
            ops.push(Op::CheckInitialized {
                slot,
                name: format!("local_{slot}"),
            });
            ops.push(Op::StoreLocal { slot, src: value });
        }
        Place::Name { name, strict } => ops.push(Op::SetName {
            key: name,
            src: value,
            strict,
        }),
        Place::Property {
            object,
            key,
            strict,
        } => emit_property_put(ops, object, key, value, strict),
        Place::Private { object, name } => ops.push(Op::SetPrivate {
            object,
            name,
            src: value,
        }),
        Place::Super { key } => emit_super_put(ops, key, value),
    }
    Some(())
}

fn emit_super_put(ops: &mut Vec<Op>, key: PlaceKey, value: u16) {
    match key {
        PlaceKey::Static(key) => ops.push(Op::SetSuperProperty { key, src: value }),
        PlaceKey::Dynamic(key) => ops.push(Op::SetSuperPropertyDynamic { key, src: value }),
    }
}

fn emit_property_put(ops: &mut Vec<Op>, object: u16, key: PlaceKey, value: u16, strict: bool) {
    match key {
        PlaceKey::Static(key) => ops.push(Op::SetProperty {
            object,
            key,
            src: value,
            strict,
        }),
        PlaceKey::Dynamic(key) => ops.push(Op::SetPropertyDynamic {
            object,
            key,
            src: value,
            strict,
        }),
    }
}

fn assignment_value(
    assignment: AssignmentOperator,
    lhs: Option<u16>,
    rhs: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    if assignment == AssignmentOperator::Assign {
        return Some(rhs);
    }
    let dst = take_register(next);
    let operator = reduce_operator(assignment.to_binary_operator()?)?;
    ops.push(Op::Binary {
        dst,
        operator,
        lhs: lhs?,
        rhs,
    });
    Some(dst)
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}
