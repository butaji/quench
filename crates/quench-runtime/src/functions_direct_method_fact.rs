pub(crate) fn direct_method_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<std::rc::Rc<crate::facts::DirectMethodFact>> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    let body = function.body.as_ref()?;
    if body.statements.is_empty() {
        return Some(std::rc::Rc::new(crate::facts::DirectMethodFact::Noop));
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

fn zero_argument_this_call<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
) -> Option<&'a str> {
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
        direct_method_fact(function).as_deref().cloned()
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
    }

    #[test]
    fn rejects_a_setter_with_a_different_source_property() {
        assert!(fact("function step(){this.destination().value=this.source().other;}").is_none());
    }
}
