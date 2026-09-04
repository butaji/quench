pub(crate) fn counted_method_loop_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<std::rc::Rc<crate::facts::CountedMethodLoopFact>> {
    bit_count_fact(function)
        .or_else(|| visit_method_loop_fact(function))
        .or_else(|| filter_method_loop_fact(function))
        .map(std::rc::Rc::new)
}

/// Recognize the conventional population-count loop as a semantic fact.  It
/// has no observable operations beyond numeric coercion, so the executor can
/// use the bounded integer operation directly for plain numeric arguments and
/// retain the complete interpreter path for all other values.
fn bit_count_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<crate::facts::CountedMethodLoopFact> {
    use oxc::ast::ast::{Expression, Statement, VariableDeclarationKind};
    use oxc::syntax::operator::{AssignmentOperator, BinaryOperator, UpdateOperator};

    let [parameter] = function.params.items.as_slice() else { return None };
    let parameter = parameter.pattern.get_identifier()?.as_str();
    let body = function.body.as_ref()?;
    let [Statement::VariableDeclaration(declaration), Statement::ForStatement(loop_statement), Statement::ReturnStatement(returned)] =
        body.statements.as_slice() else { return None };
    if declaration.kind != VariableDeclarationKind::Let
        || declaration.declarations.len() != 1
    {
        return None;
    }
    let count = declaration.declarations[0].id.get_identifier()?.as_str();
    if declaration.declarations[0].init.is_some()
        || !matches!(returned.argument.as_ref()?, Expression::Identifier(id) if id.name == count)
    {
        return None;
    }
    let Expression::AssignmentExpression(init) = loop_statement.init.as_ref()?.as_expression()? else { return None };
    if init.operator != AssignmentOperator::Assign
        || !matches!(&init.left, oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(id) if id.name == count)
        || !matches!(&init.right, Expression::NumericLiteral(number) if number.value == 0.0)
    {
        return None;
    }
    let Expression::BinaryExpression(test) = loop_statement.test.as_ref()? else { return None };
    if test.operator != BinaryOperator::GreaterThan
        || !matches!(&test.left, Expression::Identifier(id) if id.name == parameter)
        || !matches!(&test.right, Expression::NumericLiteral(number) if number.value == 0.0)
    {
        return None;
    }
    let Expression::UpdateExpression(update) = loop_statement.update.as_ref()? else { return None };
    if update.operator != UpdateOperator::Increment
        || !matches!(&update.argument, oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) if id.name == count)
    {
        return None;
    }
    let Statement::BlockStatement(loop_body) = &loop_statement.body else { return None };
    let [Statement::ExpressionStatement(statement)] = loop_body.body.as_slice() else { return None };
    let Expression::AssignmentExpression(assignment) = &statement.expression else { return None };
    if assignment.operator != AssignmentOperator::Assign
        || !matches!(&assignment.left, oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(id) if id.name == parameter)
    {
        return None;
    }
    let Expression::BinaryExpression(and) = &assignment.right else { return None };
    if and.operator != BinaryOperator::BitwiseAnd
        || !matches!(&and.left, Expression::Identifier(id) if id.name == parameter)
    {
        return None;
    }
    let Expression::BinaryExpression(subtract) = unwrap_parenthesized(&and.right) else { return None };
    (subtract.operator == BinaryOperator::Subtraction
        && matches!(&subtract.left, Expression::Identifier(id) if id.name == parameter)
        && matches!(&subtract.right, Expression::NumericLiteral(number) if number.value == 1.0))
        .then_some(crate::facts::CountedMethodLoopFact::BitCount)
}

fn unwrap_parenthesized<'a>(mut expression: &'a oxc::ast::ast::Expression<'a>) -> &'a oxc::ast::ast::Expression<'a> {
    while let oxc::ast::ast::Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    expression
}

fn visit_method_loop_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<crate::facts::CountedMethodLoopFact> {
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
    let [
        Statement::VariableDeclaration(element),
        Statement::ExpressionStatement(body_call),
    ] = loop_body.body.as_slice()
    else {
        return None;
    };
    let [element] = element.declarations.as_slice() else {
        return None;
    };
    let element_name = element.id.get_identifier()?.as_str();
    let element_method = this_call(element.init.as_ref()?, &[index])?;
    let body_method = identifier_call(&body_call.expression, element_name, &[])?;
    Some(crate::facts::CountedMethodLoopFact::Visit {
        length_method: length_method.to_string(),
        element_method: element_method.to_string(),
        body_method: body_method.to_string(),
    })
}

fn filter_method_loop_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<crate::facts::CountedMethodLoopFact> {
    use oxc::ast::ast::{Expression, ForStatementInit, SimpleAssignmentTarget, Statement};
    use oxc::syntax::operator::{BinaryOperator, LogicalOperator, UpdateOperator};
    let [source, output] = function.params.items.as_slice() else {
        return None;
    };
    let source = source.pattern.get_identifier()?.as_str();
    let output = output.pattern.get_identifier()?.as_str();
    let [
        determining,
        collection,
        Statement::ForStatement(loop_statement),
    ] = function.body.as_ref()?.statements.as_slice()
    else {
        return None;
    };
    let (determining_local, determining_property) = declared_member(determining, source)?;
    let (collection_local, collection_property) = declared_member(collection, source)?;
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
    let length_method = identifier_call(&test.right, collection_local, &[])?;
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
    let [element, Statement::IfStatement(branch)] = loop_body.body.as_slice() else {
        return None;
    };
    let (element_local, element_method) = declared_call(element, collection_local, &[index])?;
    let Expression::LogicalExpression(test) = &branch.test else {
        return None;
    };
    if test.operator != LogicalOperator::And {
        return None;
    }
    let Expression::BinaryExpression(distinct) = &test.left else {
        return None;
    };
    if distinct.operator != BinaryOperator::Inequality
        || !counted_identifier_is(&distinct.left, element_local)
        || !counted_identifier_is(&distinct.right, determining_local)
    {
        return None;
    }
    let predicate_method = identifier_call(&test.right, element_local, &[])?;
    let Statement::ExpressionStatement(append) = &branch.consequent else {
        return None;
    };
    let append_method = identifier_call(&append.expression, output, &[element_local])?;
    branch.alternate.is_none().then_some(())?;
    Some(crate::facts::CountedMethodLoopFact::Filter {
        determining_property: determining_property.to_string(),
        collection_property: collection_property.to_string(),
        length_method: length_method.to_string(),
        element_method: element_method.to_string(),
        predicate_method: predicate_method.to_string(),
        append_method: append_method.to_string(),
    })
}

fn declared_member<'a>(
    statement: &'a oxc::ast::ast::Statement<'a>,
    receiver: &str,
) -> Option<(&'a str, &'a str)> {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::VariableDeclaration(declaration) = statement else {
        return None;
    };
    let [declaration] = declaration.declarations.as_slice() else {
        return None;
    };
    let local = declaration.id.get_identifier()?.as_str();
    let Expression::StaticMemberExpression(member) = declaration.init.as_ref()? else {
        return None;
    };
    counted_identifier_is(&member.object, receiver)
        .then_some((local, member.property.name.as_str()))
}

fn declared_call<'a>(
    statement: &'a oxc::ast::ast::Statement<'a>,
    receiver: &str,
    arguments: &[&str],
) -> Option<(&'a str, &'a str)> {
    use oxc::ast::ast::Statement;
    let Statement::VariableDeclaration(declaration) = statement else {
        return None;
    };
    let [declaration] = declaration.declarations.as_slice() else {
        return None;
    };
    let local = declaration.id.get_identifier()?.as_str();
    let method = identifier_call(declaration.init.as_ref()?, receiver, arguments)?;
    Some((local, method))
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
            fact,
            crate::facts::CountedMethodLoopFact::Visit {
                length_method: "count".into(),
                element_method: "item".into(),
                body_method: "step".into(),
            }
        );
    }

    #[test]
    fn records_general_filtered_method_loop() {
        assert_eq!(
            fact(
                "function collect(v,out){var determining=v.chosen;var cc=v.items;for(var i=0;i<cc.count();i++){var x=cc.item(i);if(x!=determining&&x.ready())out.add(x);}}"
            ),
            Some(crate::facts::CountedMethodLoopFact::Filter {
                determining_property: "chosen".into(),
                collection_property: "items".into(),
                length_method: "count".into(),
                element_method: "item".into(),
                predicate_method: "ready".into(),
                append_method: "add".into(),
            })
        );
    }

    #[test]
    fn rejects_a_loop_with_an_observable_extra_statement() {
        assert!(fact("function visit(){for(var k=0;k<this.count();k++){var x=this.item(k);x.step();sideEffect();}}")
            .is_none());
    }

    #[test]
    fn records_bit_count_loop() {
        assert_eq!(
            fact("function countBits(n){let count;for(count=0;n>0;count++){n=n&(n-1);}return count;}")
                , Some(crate::facts::CountedMethodLoopFact::BitCount)
        );
    }
}
