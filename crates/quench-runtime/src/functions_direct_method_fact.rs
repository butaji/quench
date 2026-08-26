pub(crate) fn direct_method_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<std::rc::Rc<crate::facts::DirectMethodFact>> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    let body = function.body.as_ref()?;
    if body.statements.is_empty() {
        return Some(std::rc::Rc::new(crate::facts::DirectMethodFact::Noop));
    }
    if let Some(fact) = direct_return_fact(body.statements.as_slice(), locals) {
        return Some(std::rc::Rc::new(fact));
    }
    if let Some(property) = append_array_fact(function, body.statements.as_slice()) {
        return Some(std::rc::Rc::new(
            crate::facts::DirectMethodFact::AppendArray {
                property: property.to_string(),
            },
        ));
    }
    let [Statement::ExpressionStatement(statement)] = body.statements.as_slice() else {
        return None;
    };
    let Expression::AssignmentExpression(assignment) = &statement.expression else {
        return None;
    };
    if assignment.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(target) = &assignment.left else {
        return None;
    };
    let Expression::StaticMemberExpression(source) = &assignment.right else {
        return None;
    };
    if target.property.name != source.property.name {
        return None;
    }
    let target_method = zero_argument_this_call(&target.object)?;
    let source_method = zero_argument_this_call(&source.object)?;
    Some(std::rc::Rc::new(
        crate::facts::DirectMethodFact::CopyMethodProperty {
            target_method: target_method.to_string(),
            source_method: source_method.to_string(),
            property: target.property.name.to_string(),
        },
    ))
}

fn direct_return_fact(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::DirectMethodFact> {
    use oxc::ast::ast::{Expression, Statement};
    let [Statement::ReturnStatement(returned)] = statements else {
        return None;
    };
    let returned = returned.argument.as_ref()?;
    if let Some(property) = direct_this_member(returned) {
        return Some(crate::facts::DirectMethodFact::PropertyLoad {
            property: property.to_string(),
        });
    }
    let Expression::BinaryExpression(binary) = returned else {
        return None;
    };
    if binary.operator != oxc::syntax::operator::BinaryOperator::Inequality {
        return None;
    }
    let property = direct_this_member(&binary.left)?;
    let Expression::StaticMemberExpression(capture) = &binary.right else {
        return None;
    };
    let Expression::Identifier(capture_object) = &capture.object else {
        return None;
    };
    Some(crate::facts::DirectMethodFact::PropertyNotEqualCapture {
        property: property.to_string(),
        capture_slot: *locals.get(capture_object.name.as_str())?,
        capture_property: capture.property.name.to_string(),
    })
}

fn append_array_fact<'a>(
    function: &oxc::ast::ast::Function<'a>,
    statements: &'a [oxc::ast::ast::Statement<'a>],
) -> Option<&'a str> {
    use oxc::ast::ast::{Expression, Statement};
    let [parameter] = function.params.items.as_slice() else {
        return None;
    };
    let parameter = parameter.pattern.get_identifier()?.as_str();
    let [Statement::ExpressionStatement(statement)] = statements else {
        return None;
    };
    let Expression::CallExpression(call) = &statement.expression else {
        return None;
    };
    let Expression::StaticMemberExpression(method) = &call.callee else {
        return None;
    };
    if method.property.name != "push" {
        return None;
    }
    let property = direct_this_member(&method.object)?;
    let [argument] = call.arguments.as_slice() else {
        return None;
    };
    argument
        .as_expression()
        .is_some_and(|argument| direct_identifier_is(argument, parameter))
        .then_some(property)
}

fn direct_this_member<'a>(expression: &'a oxc::ast::ast::Expression<'a>) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::StaticMemberExpression(member) = expression else {
        return None;
    };
    matches!(member.object, Expression::ThisExpression(_)).then_some(member.property.name.as_str())
}

fn direct_identifier_is(expression: &oxc::ast::ast::Expression<'_>, expected: &str) -> bool {
    matches!(expression, oxc::ast::ast::Expression::Identifier(identifier) if identifier.name == expected)
}

fn zero_argument_this_call<'a>(expression: &'a oxc::ast::ast::Expression<'a>) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    let Expression::StaticMemberExpression(method) = &call.callee else {
        return None;
    };
    (call.arguments.is_empty() && matches!(method.object, Expression::ThisExpression(_)))
        .then_some(method.property.name.as_str())
}

#[cfg(test)]
mod direct_method_fact_tests {
    use super::direct_method_fact;
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn fact(source: &str) -> Option<crate::facts::DirectMethodFact> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        direct_method_fact(function, &std::collections::HashMap::new())
            .as_deref()
            .cloned()
    }

    #[test]
    fn records_empty_and_copy_methods() {
        assert_eq!(
            fact("function step(){}"),
            Some(crate::facts::DirectMethodFact::Noop)
        );
        assert_eq!(
            fact("function step(){this.destination().value=this.source().value;}"),
            Some(crate::facts::DirectMethodFact::CopyMethodProperty {
                target_method: "destination".into(),
                source_method: "source".into(),
                property: "value".into(),
            })
        );
        assert_eq!(
            fact("function add(value){this.values.push(value);}"),
            Some(crate::facts::DirectMethodFact::AppendArray {
                property: "values".into(),
            })
        );
        assert_eq!(
            fact("function ready(){return this.satisfied;}"),
            Some(crate::facts::DirectMethodFact::PropertyLoad {
                property: "satisfied".into(),
            })
        );
    }

    #[test]
    fn rejects_a_setter_with_a_different_source_property() {
        assert!(fact("function step(){this.destination().value=this.source().other;}").is_none());
    }
}
