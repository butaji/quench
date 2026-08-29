pub(crate) fn forward_then_call_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<std::rc::Rc<crate::facts::ForwardThenCallFact>> {
    use oxc::ast::ast::{Expression, Statement};
    let body = function.body.as_ref()?;
    let [Statement::ExpressionStatement(first), Statement::ExpressionStatement(second)] =
        body.statements.as_slice()
    else {
        return None;
    };
    let Expression::CallExpression(first) = &first.expression else {
        return None;
    };
    let first_method = direct_this_method(&first.callee)?;
    let parameters = parameter_names(function)?;
    if first.arguments.len() != parameters.len() {
        return None;
    }
    for (argument, parameter) in first.arguments.iter().zip(parameters) {
        let Expression::Identifier(identifier) = argument.as_expression()? else {
            return None;
        };
        if identifier.name != parameter {
            return None;
        }
    }
    let Expression::CallExpression(second) = &second.expression else {
        return None;
    };
    if !second.arguments.is_empty() {
        return None;
    }
    let (nested_property, nested_method) = nested_this_method(&second.callee)?;
    Some(std::rc::Rc::new(crate::facts::ForwardThenCallFact {
        first_method: first_method.to_string(),
        nested_property: nested_property.to_string(),
        nested_method: nested_method.to_string(),
    }))
}

fn direct_this_method<'a>(expression: &'a oxc::ast::ast::Expression<'a>) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::StaticMemberExpression(member) = expression else {
        return None;
    };
    matches!(member.object, Expression::ThisExpression(_)).then_some(member.property.name.as_str())
}

fn nested_this_method<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
) -> Option<(&'a str, &'a str)> {
    use oxc::ast::ast::Expression;
    let Expression::StaticMemberExpression(method) = expression else {
        return None;
    };
    let Expression::StaticMemberExpression(property) = &method.object else {
        return None;
    };
    matches!(property.object, Expression::ThisExpression(_)).then_some((
        property.property.name.as_str(),
        method.property.name.as_str(),
    ))
}

#[cfg(test)]
mod forward_then_call_fact_tests {
    use super::forward_then_call_fact;
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    #[test]
    fn records_forward_then_nested_call() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function add(a,b){this.put(a,b);this.current.start();}",
            SourceType::default(),
        )
        .parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function");
        };
        let fact = forward_then_call_fact(function).unwrap();
        assert_eq!(
            (
                fact.first_method.as_str(),
                fact.nested_property.as_str(),
                fact.nested_method.as_str()
            ),
            ("put", "current", "start")
        );
    }
}
