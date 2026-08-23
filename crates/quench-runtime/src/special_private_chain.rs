fn reduce_private_chain(
    member: &oxc::ast::ast::PrivateFieldExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let name = facts.private_name(member.field.span)?;
    if let Expression::StaticMemberExpression(object) = &member.object {
        if object.optional {
            return reduce_private_after_optional_member(object, name, ops, facts, next, locals);
        }
    }
    let object = crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    if member.optional {
        emit_optional_private(ops, dst, object, name);
    } else {
        ops.push(Op::GetPrivate {
            dst,
            object,
            name,
        });
    }
    Some(dst)
}

fn reduce_private_after_optional_member(
    member: &oxc::ast::ast::StaticMemberExpression<'_>,
    name: crate::facts::PrivateNameId,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let base = crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    let condition = *next;
    *next = next.saturating_add(1);
    ops.push(Op::Unary {
        dst: condition,
        operator: crate::ops::UnaryOp::IsNullish,
        src: base,
    });
    let then_ops = vec![Op::Const {
        dst,
        value: crate::ops::Constant::Undefined,
    }];
    let property = *next;
    *next = next.saturating_add(1);
    let else_ops = vec![
        Op::GetProperty {
            dst: property,
            object: base,
            key: member.property.name.to_string(),
        },
        Op::GetPrivate {
            dst,
            object: property,
            name,
        },
    ];
    let mut branches = crate::machine::FunctionCode::from_ops_many(vec![then_ops, else_ops]);
    ops.push(Op::Branch {
        condition,
        then_ops: branches.remove(0),
        else_ops: branches.remove(0),
    });
    Some(dst)
}

fn emit_optional_private(
    ops: &mut Vec<Op>,
    dst: u16,
    object: u16,
    name: crate::facts::PrivateNameId,
) {
    ops.push(Op::OptionalGetPrivate { dst, object, name });
}
