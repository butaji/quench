pub(crate) fn linked_record_insert_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<std::rc::Rc<crate::facts::LinkedRecordInsertFact>> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};

    let body = function.body.as_ref()?;
    let [first, second, third] = body.statements.as_slice() else {
        return None;
    };
    let (_, count) = crate::function_parameters::bindings(&function.params).ok()?;
    if count != 4 {
        return None;
    }
    let [id, priority, queue, task]: [&str; 4] = function
        .params
        .items
        .iter()
        .map(|parameter| {
            parameter
                .pattern
                .get_identifier()
                .map(|identifier| identifier.as_str())
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;

    let (current, first_value) = static_this_assignment(first)?;
    let Expression::NewExpression(created) = first_value else {
        return None;
    };
    let Expression::Identifier(constructor) = &created.callee else {
        return None;
    };
    let [list_arg, id_arg, priority_arg, queue_arg, task_arg] = created.arguments.as_slice() else {
        return None;
    };
    let list = this_member(list_arg.as_expression()?)?;
    if !identifier_is(id_arg.as_expression()?, id)
        || !identifier_is(priority_arg.as_expression()?, priority)
        || !identifier_is(queue_arg.as_expression()?, queue)
        || !identifier_is(task_arg.as_expression()?, task)
    {
        return None;
    }

    let (second_target, second_value) = static_this_assignment(second)?;
    if second_target != list || this_member(second_value)? != current {
        return None;
    }

    let Statement::ExpressionStatement(third) = third else {
        return None;
    };
    let Expression::AssignmentExpression(third) = &third.expression else {
        return None;
    };
    let AssignmentTarget::ComputedMemberExpression(indexed) = &third.left else {
        return None;
    };
    let index = this_member(&indexed.object)?;
    if !identifier_is(&indexed.expression, id) || this_member(&third.right)? != current {
        return None;
    }

    Some(std::rc::Rc::new(crate::facts::LinkedRecordInsertFact {
        constructor_slot: *locals.get(constructor.name.as_str())?,
        current: current.to_string(),
        list: list.to_string(),
        index: index.to_string(),
    }))
}

fn static_this_assignment<'a>(
    statement: &'a oxc::ast::ast::Statement<'a>,
) -> Option<(&'a str, &'a oxc::ast::ast::Expression<'a>)> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    use oxc::syntax::operator::AssignmentOperator;
    let Statement::ExpressionStatement(statement) = statement else {
        return None;
    };
    let Expression::AssignmentExpression(assignment) = &statement.expression else {
        return None;
    };
    if assignment.operator != AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(member) = &assignment.left else {
        return None;
    };
    matches!(member.object, Expression::ThisExpression(_))
        .then_some((member.property.name.as_str(), &assignment.right))
}

fn this_member<'a>(expression: &'a oxc::ast::ast::Expression<'a>) -> Option<&'a str> {
    use oxc::ast::ast::Expression;
    let Expression::StaticMemberExpression(member) = expression else {
        return None;
    };
    matches!(member.object, Expression::ThisExpression(_)).then_some(member.property.name.as_str())
}

fn identifier_is(expression: &oxc::ast::ast::Expression<'_>, expected: &str) -> bool {
    matches!(expression, oxc::ast::ast::Expression::Identifier(identifier) if identifier.name == expected)
}

#[cfg(test)]
mod linked_record_insert_fact_tests {
    use super::linked_record_insert_fact;
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn fact(source: &str) -> Option<crate::facts::LinkedRecordInsertFact> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        linked_record_insert_fact(
            function,
            &std::collections::HashMap::from([("Node".to_string(), 7)]),
        )
        .as_deref()
        .cloned()
    }

    #[test]
    fn records_linked_record_insertion_as_data() {
        let fact = fact(
            "function add(id,p,q,t){this.cur=new Node(this.list,id,p,q,t);this.list=this.cur;this.blocks[id]=this.cur;}",
        )
        .unwrap();
        assert_eq!(fact.constructor_slot, 7);
        assert_eq!(
            (
                fact.current.as_str(),
                fact.list.as_str(),
                fact.index.as_str()
            ),
            ("cur", "list", "blocks")
        );
    }

    #[test]
    fn rejects_a_different_assignment_order() {
        assert!(fact(
            "function add(id,p,q,t){this.cur=new Node(this.list,id,p,q,t);this.blocks[id]=this.cur;this.list=this.cur;}"
        )
        .is_none());
    }
}
