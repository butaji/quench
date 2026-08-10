fn default_value(
    source: u16,
    fallback: &Expression<'_>,
    name: Option<&str>,
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
    let consequent = fallback_ops(fallback, name, facts, next, locals)?;
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
    name: Option<&str>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Vec<Op>> {
    let mut ops = Vec::new();
    let value = crate::reduce::reduce_expression(fallback, &mut ops, facts, next, locals)?;
    if anonymous_function_definition(fallback) {
        if let Some(name) = name {
            let name = emit_const(&mut ops, next, Constant::String(name.to_string()));
            ops.push(Op::SetProperty {
                object: value,
                key: "name".to_string(),
                src: name,
            });
        }
    }
    ops.push(Op::Return { src: value });
    Some(ops)
}

fn anonymous_function_definition(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ArrowFunctionExpression(_) => true,
        Expression::FunctionExpression(function) => function.id.is_none(),
        Expression::ClassExpression(class) => {
            class.id.is_none() && !class_has_static_name(class.as_ref())
        }
        Expression::ParenthesizedExpression(expression) => {
            anonymous_function_definition(&expression.expression)
        }
        _ => false,
    }
}

fn class_has_static_name(class: &oxc::ast::ast::Class<'_>) -> bool {
    class.body.body.iter().any(|element| match element {
        oxc::ast::ast::ClassElement::MethodDefinition(method) => {
            method.r#static && method.key.static_name().is_some_and(|name| name == "name")
        }
        oxc::ast::ast::ClassElement::PropertyDefinition(field) => {
            field.r#static && field.key.static_name().is_some_and(|name| name == "name")
        }
        _ => false,
    })
}

fn assignment_name<'a>(target: &'a AssignmentTarget<'_>) -> Option<&'a str> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(identifier) => Some(identifier.name.as_str()),
        _ => None,
    }
}
