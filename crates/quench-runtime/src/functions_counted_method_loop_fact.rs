pub(crate) fn counted_method_loop_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<std::rc::Rc<crate::facts::CountedMethodLoopFact>> {
    use oxc::{
        ast::ast::{Expression, ForStatementInit, SimpleAssignmentTarget, Statement},
        syntax::operator::{BinaryOperator, UpdateOperator},
    };

    let body = function.body.as_ref()?;
    let [Statement::ForStatement(loop_statement)] = body.statements.as_slice() else {
        return None;
    };
    let ForStatementInit::VariableDeclaration(initializer) = loop_statement.init.as_ref()? else {
        return None;
    };
    let [initial] = initializer.declarations.as_slice() else {
        return None;
    };
    let index = initial.id.get_identifier()?.as_str();
    if !matches!(initial.init.as_ref()?, Expression::NumericLiteral(value) if value.value == 0.0) {
        return None;
    }
    let Expression::BinaryExpression(test) = loop_statement.test.as_ref()? else {
        return None;
    };
    if test.operator != BinaryOperator::LessThan || !counted_identifier_is(&test.left, index) {
        return None;
    }
    let length_method = this_call(&test.right, &[])?;
    let Expression::UpdateExpression(update) = loop_statement.update.as_ref()? else {
        return None;
    };
    if update.operator != UpdateOperator::Increment
        || !matches!(&update.argument, SimpleAssignmentTarget::AssignmentTargetIdentifier(id) if id.name == index)
    {
        return None;
    }
    let Statement::BlockStatement(loop_body) = &loop_statement.body else {
        return None;
    };
    let [Statement::VariableDeclaration(element), Statement::ExpressionStatement(body_call)] =
        loop_body.body.as_slice()
    else {
        return None;
    };
    let [element] = element.declarations.as_slice() else {
        return None;
    };
    let element_name = element.id.get_identifier()?.as_str();
    let element_method = this_call(element.init.as_ref()?, &[index])?;
    let body_method = identifier_call(&body_call.expression, element_name, &[])?;
    Some(std::rc::Rc::new(crate::facts::CountedMethodLoopFact {
        length_method: length_method.to_string(),
        element_method: element_method.to_string(),
        body_method: body_method.to_string(),
    }))
}

fn this_call<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
    arguments: &[&str],
) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if !matches!(member.object, Expression::ThisExpression(_))
        || call.arguments.len() != arguments.len()
    {
        return None;
    }
    call.arguments
        .iter()
        .zip(arguments)
        .all(|(argument, expected)| {
            argument
                .as_expression()
                .is_some_and(|argument| counted_identifier_is(argument, expected))
        })
        .then_some(member.property.name.as_str())
}

fn identifier_call<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
    receiver: &str,
    arguments: &[&str],
) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if !counted_identifier_is(&member.object, receiver) || call.arguments.len() != arguments.len() {
        return None;
    }
    call.arguments
        .iter()
        .zip(arguments)
        .all(|(argument, expected)| {
            argument
                .as_expression()
                .is_some_and(|argument| counted_identifier_is(argument, expected))
        })
        .then_some(member.property.name.as_str())
}

fn counted_identifier_is(expression: &oxc::ast::ast::Expression<'_>, expected: &str) -> bool {
    matches!(expression, oxc::ast::ast::Expression::Identifier(identifier) if identifier.name == expected)
}

#[cfg(test)]
mod counted_method_loop_fact_tests {
    use super::counted_method_loop_fact;
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn fact(source: &str) -> Option<crate::facts::CountedMethodLoopFact> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        counted_method_loop_fact(function).as_deref().cloned()
    }

    #[test]
    fn records_general_counted_method_loop() {
        let fact =
            fact("function visit(){for(var k=0;k<this.count();k++){var x=this.item(k);x.step();}}")
                .unwrap();
        assert_eq!(
            (
                fact.length_method.as_str(),
                fact.element_method.as_str(),
                fact.body_method.as_str()
            ),
            ("count", "item", "step")
        );
    }

    #[test]
    fn rejects_a_loop_with_an_observable_extra_statement() {
        assert!(fact("function visit(){for(var k=0;k<this.count();k++){var x=this.item(k);x.step();sideEffect();}}")
            .is_none());
    }
}
