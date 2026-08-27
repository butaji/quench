pub(crate) fn direct_method_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<std::rc::Rc<crate::facts::DirectMethodFact>> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    let body = function.body.as_ref()?;
    if body.statements.is_empty() {
        return Some(std::rc::Rc::new(crate::facts::DirectMethodFact::Noop));
    }
    if let Some(fact) = select_update_call_fact(body.statements.as_slice(), locals) {
        return Some(std::rc::Rc::new(fact));
    }
    if let Some(fact) = direct_return_fact(function, body.statements.as_slice(), locals) {
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

fn select_update_call_fact(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::DirectMethodFact> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    let [Statement::VariableDeclaration(bindings), value_update, flag_update, Statement::IfStatement(run)] =
        statements
    else {
        return None;
    };
    let [input, output] = bindings.declarations.as_slice() else {
        return None;
    };
    let input_local = input.id.get_identifier()?.as_str();
    let output_local = output.id.get_identifier()?.as_str();
    let input_method = zero_argument_this_call(input.init.as_ref()?)?;
    let output_method = zero_argument_this_call(output.init.as_ref()?)?;

    let Statement::ExpressionStatement(value_update) = value_update else {
        return None;
    };
    let Expression::AssignmentExpression(value_update) = &value_update.expression else {
        return None;
    };
    if value_update.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(output_value_target) = &value_update.left else {
        return None;
    };
    let Expression::Identifier(output_value_object) = &output_value_target.object else {
        return None;
    };
    (output_value_object.name == output_local).then_some(())?;
    let Expression::CallExpression(combine) = &value_update.right else {
        return None;
    };
    let Expression::StaticMemberExpression(combine_member) = &combine.callee else {
        return None;
    };
    let Expression::Identifier(namespace_object) = &combine_member.object else {
        return None;
    };
    let [receiver_value, input_value] = combine.arguments.as_slice() else {
        return None;
    };
    let receiver_value = direct_this_member(receiver_value.as_expression()?)?;
    let Expression::StaticMemberExpression(input_value) = input_value.as_expression()? else {
        return None;
    };
    let Expression::Identifier(input_value_object) = &input_value.object else {
        return None;
    };
    (input_value_object.name == input_local).then_some(())?;

    let Statement::ExpressionStatement(flag_update) = flag_update else {
        return None;
    };
    let Expression::AssignmentExpression(flag_update) = &flag_update.expression else {
        return None;
    };
    if flag_update.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(output_flag_target) = &flag_update.left else {
        return None;
    };
    let Expression::Identifier(output_flag_object) = &output_flag_target.object else {
        return None;
    };
    (output_flag_object.name == output_local).then_some(())?;
    let mut flag_terms = Vec::new();
    flatten_and_terms(&flag_update.right, &mut flag_terms);
    let mut input_flag = None;
    let mut extra_flag_fields = Vec::new();
    for term in flag_terms {
        let Expression::StaticMemberExpression(member) = term else {
            return None;
        };
        match &member.object {
            Expression::Identifier(identifier) if identifier.name == input_local => {
                input_flag = Some(member.property.name.to_string());
            }
            Expression::StaticMemberExpression(owner)
                if matches!(owner.object, Expression::ThisExpression(_)) =>
            {
                extra_flag_fields.push((
                    owner.property.name.to_string(),
                    member.property.name.to_string(),
                ));
            }
            _ => return None,
        }
    }
    let input_flag = input_flag?;
    if extra_flag_fields
        .iter()
        .any(|(_, property)| property != &input_flag)
    {
        return None;
    }
    let extra_flag_objects = extra_flag_fields
        .into_iter()
        .map(|(owner, _)| owner)
        .collect();

    let Expression::StaticMemberExpression(test) = &run.test else {
        return None;
    };
    let Expression::Identifier(test_object) = &test.object else {
        return None;
    };
    (test_object.name == output_local && test.property.name == output_flag_target.property.name)
        .then_some(())?;
    let Statement::ExpressionStatement(execute) = &run.consequent else {
        return None;
    };
    let conditional_method = zero_argument_this_call(&execute.expression)?;
    run.alternate.is_none().then_some(())?;

    Some(crate::facts::DirectMethodFact::SelectUpdateCall {
        input_method: input_method.to_string(),
        output_method: output_method.to_string(),
        namespace_slot: *locals.get(namespace_object.name.as_str())?,
        combine_method: combine_member.property.name.to_string(),
        receiver_value: receiver_value.to_string(),
        input_value: input_value.property.name.to_string(),
        output_value: output_value_target.property.name.to_string(),
        input_flag,
        output_flag: output_flag_target.property.name.to_string(),
        extra_flag_objects,
        conditional_method: conditional_method.to_string(),
    })
}

fn flatten_and_terms<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
    output: &mut Vec<&'a oxc::ast::ast::Expression<'a>>,
) {
    if let oxc::ast::ast::Expression::LogicalExpression(logical) = expression {
        if logical.operator == oxc::syntax::operator::LogicalOperator::And {
            flatten_and_terms(&logical.left, output);
            flatten_and_terms(&logical.right, output);
            return;
        }
    }
    output.push(expression);
}

fn direct_return_fact(
    function: &oxc::ast::ast::Function<'_>,
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::DirectMethodFact> {
    use oxc::ast::ast::{Expression, Statement};
    let [Statement::ReturnStatement(returned)] = statements else {
        return None;
    };
    let returned = returned.argument.as_ref()?;
    if let Some((receiver, argument)) = slot_dot3_fact(function, returned) {
        return Some(crate::facts::DirectMethodFact::SlotDot3 { receiver, argument });
    }
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

fn slot_dot3_fact(
    function: &oxc::ast::ast::Function<'_>,
    expression: &oxc::ast::ast::Expression<'_>,
) -> Option<([String; 3], [String; 3])> {
    use oxc::ast::ast::Expression;
    let [parameter] = function.params.items.as_slice() else {
        return None;
    };
    let parameter = parameter.pattern.get_identifier()?.as_str();
    let mut terms = Vec::new();
    flatten_add_terms(expression, &mut terms);
    let terms: [&Expression<'_>; 3] = terms.try_into().ok()?;
    let pairs: Vec<_> = terms
        .into_iter()
        .map(|term| slot_product(term, parameter))
        .collect::<Option<_>>()?;
    Some((
        std::array::from_fn(|index| pairs[index].0.to_string()),
        std::array::from_fn(|index| pairs[index].1.to_string()),
    ))
}

fn flatten_add_terms<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
    output: &mut Vec<&'a oxc::ast::ast::Expression<'a>>,
) {
    if let oxc::ast::ast::Expression::BinaryExpression(binary) = expression {
        if binary.operator == oxc::syntax::operator::BinaryOperator::Addition {
            flatten_add_terms(&binary.left, output);
            flatten_add_terms(&binary.right, output);
            return;
        }
    }
    output.push(expression);
}

fn slot_product<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
    parameter: &str,
) -> Option<(&'a str, &'a str)> {
    use oxc::ast::ast::Expression;
    let Expression::BinaryExpression(product) = expression else {
        return None;
    };
    (product.operator == oxc::syntax::operator::BinaryOperator::Multiplication).then_some(())?;
    let receiver = direct_this_member(&product.left)?;
    let Expression::StaticMemberExpression(argument) = &product.right else {
        return None;
    };
    let Expression::Identifier(owner) = &argument.object else {
        return None;
    };
    (owner.name == parameter).then_some((receiver, argument.property.name.as_str()))
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
        assert_eq!(
            fact("function project(other){return this.a*other.u+this.b*other.v+this.c*other.w;}"),
            Some(crate::facts::DirectMethodFact::SlotDot3 {
                receiver: ["a".into(), "b".into(), "c".into()],
                argument: ["u".into(), "v".into(), "w".into()],
            })
        );
    }

    #[test]
    fn rejects_a_setter_with_a_different_source_property() {
        assert!(fact("function step(){this.destination().value=this.source().other;}").is_none());
    }

    #[test]
    fn records_select_update_call_with_optional_flag_dependencies() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function recalc(){var i=this.input(),o=this.output();o.rank=Rank.min(this.rank,i.rank);o.live=i.live&&this.scale.live;if(o.live)this.execute();}",
            SourceType::default(),
        )
        .parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        let locals = std::collections::HashMap::from([("Rank".to_string(), 7)]);
        assert!(matches!(
            direct_method_fact(function, &locals).as_deref(),
            Some(crate::facts::DirectMethodFact::SelectUpdateCall {
                namespace_slot: 7,
                input_method,
                output_method,
                extra_flag_objects,
                ..
            }) if input_method == "input"
                && output_method == "output"
                && extra_flag_objects == &["scale"]
        ));
    }
}
