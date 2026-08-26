pub(crate) fn direct_method_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<std::rc::Rc<crate::facts::DirectMethodFact>> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    let body = function.body.as_ref()?;
    if body.statements.is_empty() {
        return Some(std::rc::Rc::new(crate::facts::DirectMethodFact::Noop));
    }
    if let Some(fact) = recalculate_fact(body.statements.as_slice(), locals) {
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

fn recalculate_fact(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::DirectMethodFact> {
    use oxc::ast::ast::{AssignmentTarget, Expression, Statement};
    let [Statement::VariableDeclaration(bindings), strength, stay, Statement::IfStatement(run)] =
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

    let Statement::ExpressionStatement(strength) = strength else {
        return None;
    };
    let Expression::AssignmentExpression(strength) = &strength.expression else {
        return None;
    };
    if strength.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(output_strength_target) = &strength.left else {
        return None;
    };
    let Expression::Identifier(output_strength_object) = &output_strength_target.object else {
        return None;
    };
    (output_strength_object.name == output_local).then_some(())?;
    let Expression::CallExpression(weakest) = &strength.right else {
        return None;
    };
    let Expression::StaticMemberExpression(weakest_member) = &weakest.callee else {
        return None;
    };
    let Expression::Identifier(strength_object) = &weakest_member.object else {
        return None;
    };
    let [receiver_strength, input_strength] = weakest.arguments.as_slice() else {
        return None;
    };
    let receiver_strength = receiver_strength.as_expression()?;
    let receiver_strength = direct_this_member(receiver_strength)?;
    let input_strength = input_strength.as_expression()?;
    let Expression::StaticMemberExpression(input_strength) = input_strength else {
        return None;
    };
    let Expression::Identifier(input_strength_object) = &input_strength.object else {
        return None;
    };
    (input_strength_object.name == input_local).then_some(())?;

    let Statement::ExpressionStatement(stay) = stay else {
        return None;
    };
    let Expression::AssignmentExpression(stay) = &stay.expression else {
        return None;
    };
    if stay.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(output_stay_target) = &stay.left else {
        return None;
    };
    let Expression::Identifier(output_stay_object) = &output_stay_target.object else {
        return None;
    };
    (output_stay_object.name == output_local).then_some(())?;
    let mut stay_terms = Vec::new();
    flatten_and_terms(&stay.right, &mut stay_terms);
    let mut input_stay = None;
    let mut extra_stay_fields = Vec::new();
    for term in stay_terms {
        let Expression::StaticMemberExpression(member) = term else {
            return None;
        };
        match &member.object {
            Expression::Identifier(identifier) if identifier.name == input_local => {
                input_stay = Some(member.property.name.to_string());
            }
            Expression::StaticMemberExpression(owner)
                if matches!(owner.object, Expression::ThisExpression(_)) =>
            {
                extra_stay_fields.push((
                    owner.property.name.to_string(),
                    member.property.name.to_string(),
                ));
            }
            _ => return None,
        }
    }
    let input_stay = input_stay?;
    if extra_stay_fields
        .iter()
        .any(|(_, property)| property != &input_stay)
    {
        return None;
    }
    let extra_stay_objects = extra_stay_fields
        .into_iter()
        .map(|(owner, _)| owner)
        .collect();

    let Expression::StaticMemberExpression(test) = &run.test else {
        return None;
    };
    let Expression::Identifier(test_object) = &test.object else {
        return None;
    };
    (test_object.name == output_local && test.property.name == output_stay_target.property.name)
        .then_some(())?;
    let Statement::ExpressionStatement(execute) = &run.consequent else {
        return None;
    };
    let execute_method = zero_argument_this_call(&execute.expression)?;
    run.alternate.is_none().then_some(())?;

    Some(crate::facts::DirectMethodFact::Recalculate {
        input_method: input_method.to_string(),
        output_method: output_method.to_string(),
        strength_slot: *locals.get(strength_object.name.as_str())?,
        weakest_method: weakest_member.property.name.to_string(),
        receiver_strength: receiver_strength.to_string(),
        input_strength: input_strength.property.name.to_string(),
        output_strength: output_strength_target.property.name.to_string(),
        input_stay,
        output_stay: output_stay_target.property.name.to_string(),
        extra_stay_objects,
        execute_method: execute_method.to_string(),
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
    fn records_recalculate_shape_with_optional_stay_dependencies() {
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
            Some(crate::facts::DirectMethodFact::Recalculate {
                strength_slot: 7,
                input_method,
                output_method,
                extra_stay_objects,
                ..
            }) if input_method == "input"
                && output_method == "output"
                && extra_stay_objects == &["scale"]
        ));
    }
}
