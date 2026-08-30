pub(crate) fn forward_construct_call_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<std::rc::Rc<crate::facts::ForwardConstructCallFact>> {
    use oxc::ast::ast::{Expression, Statement};
    let body = function.body.as_ref()?;
    let [Statement::ExpressionStatement(statement)] = body.statements.as_slice() else {
        return None;
    };
    let Expression::CallExpression(call) = &statement.expression else {
        return None;
    };
    let method = forward_this_member(&call.callee)?;
    let parameters = parameter_names(function)?;
    if call.arguments.is_empty() {
        return None;
    }
    let forwarded_arguments = call.arguments[..call.arguments.len() - 1]
        .iter()
        .map(|argument| parameter_index(argument.as_expression()?, &parameters))
        .collect::<Option<Vec<_>>>()?;
    let Expression::NewExpression(created) = call.arguments.last()?.as_expression()? else {
        return None;
    };
    let Expression::Identifier(constructor) = &created.callee else {
        return None;
    };
    let constructor_arguments = created
        .arguments
        .iter()
        .map(|argument| value_source(argument.as_expression()?, &parameters, locals))
        .collect::<Option<Vec<_>>>()?;
    Some(std::rc::Rc::new(crate::facts::ForwardConstructCallFact {
        method: method.to_string(),
        constructor_slot: *locals.get(constructor.name.as_str())?,
        forwarded_arguments: forwarded_arguments.into(),
        constructor_arguments: constructor_arguments.into(),
    }))
}

fn parameter_names<'a>(function: &'a oxc::ast::ast::Function<'a>) -> Option<Vec<&'a str>> {
    function
        .params
        .items
        .iter()
        .map(|parameter| parameter.pattern.get_identifier().map(|name| name.as_str()))
        .collect()
}

fn value_source(
    expression: &oxc::ast::ast::Expression<'_>,
    parameters: &[&str],
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::ForwardValueSource> {
    use oxc::ast::ast::Expression;
    match expression {
        Expression::ThisExpression(_) => Some(crate::facts::ForwardValueSource::Receiver),
        Expression::Identifier(identifier) => parameters
            .iter()
            .position(|name| *name == identifier.name)
            .map(|index| crate::facts::ForwardValueSource::Argument(index as u16))
            .or_else(|| {
                locals
                    .get(identifier.name.as_str())
                    .copied()
                    .map(crate::facts::ForwardValueSource::Capture)
            }),
        Expression::NumericLiteral(value)
            if value.value.fract() == 0.0
                && value.value >= i32::MIN as f64
                && value.value <= i32::MAX as f64 =>
        {
            Some(crate::facts::ForwardValueSource::Integer(
                value.value as i32,
            ))
        }
        _ => None,
    }
}

fn forward_this_member<'a>(expression: &'a oxc::ast::ast::Expression<'a>) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::StaticMemberExpression(member) = expression else {
        return None;
    };
    matches!(member.object, Expression::ThisExpression(_)).then_some(member.property.name.as_str())
}

fn parameter_index(expression: &oxc::ast::ast::Expression<'_>, parameters: &[&str]) -> Option<u16> {
    let oxc::ast::ast::Expression::Identifier(identifier) = expression else {
        return None;
    };
    parameters
        .iter()
        .position(|name| *name == identifier.name)
        .map(|index| index as u16)
}

#[cfg(test)]
mod forward_construct_call_fact_tests {
    use super::forward_construct_call_fact;
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn fact(source: &str) -> Option<crate::facts::ForwardConstructCallFact> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        forward_construct_call_fact(
            function,
            &std::collections::HashMap::from([("Node".to_string(), 7), ("SEED".to_string(), 9)]),
        )
        .as_deref()
        .cloned()
    }

    #[test]
    fn records_forwarded_arguments_and_constructor_sources() {
        use crate::facts::ForwardValueSource::*;
        let fact = fact("function add(id,q){this.put(id,q,new Node(this,SEED,0,q));}").unwrap();
        assert_eq!(fact.method, "put");
        assert_eq!(fact.constructor_slot, 7);
        assert_eq!(&*fact.forwarded_arguments, &[0, 1]);
        assert_eq!(
            &*fact.constructor_arguments,
            &[Receiver, Capture(9), Integer(0), Argument(1)]
        );
    }

    #[test]
    fn rejects_non_forwarded_call_arguments() {
        assert!(fact("function add(id,q){this.put(id + 1,q,new Node(this));}").is_none());
    }
}
