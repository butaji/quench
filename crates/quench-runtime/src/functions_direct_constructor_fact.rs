const DIRECT_CONSTRUCTOR_FIELD_LIMIT: usize = 8;

pub(crate) fn direct_constructor_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> std::rc::Rc<[crate::facts::DirectConstructorField]> {
    let Some(body) = function.body.as_ref() else {
        return std::rc::Rc::default();
    };
    let Ok((parameters, _)) = crate::function_parameters::bindings(&function.params) else {
        return std::rc::Rc::default();
    };
    let mut fields = Vec::new();
    // Parameter-normalization statements are facts about the value later
    // stored in a field. Keep them in the extractor instead of making the
    // runtime replay the branch and assignment for every construction.
    let mut falsy_fallbacks = std::collections::HashMap::new();
    for statement in &body.statements {
        if fields.is_empty() {
            if let Some((argument, fallback)) =
                direct_parameter_falsy_fallback(statement, &parameters)
            {
                if falsy_fallbacks.insert(argument, fallback).is_some() {
                    return std::rc::Rc::default();
                }
                continue;
            }
        }
        let field = if let Some((name, expression)) = direct_constructor_assignment(statement) {
            let Some(source) = direct_constructor_source_with_fallbacks(
                expression,
                &parameters,
                locals,
                &falsy_fallbacks,
            ) else {
                return std::rc::Rc::default();
            };
            crate::facts::DirectConstructorField {
                name: name.to_string(),
                source,
            }
        } else {
            let Some(field) = direct_constructor_conditional(statement, &parameters, locals) else {
                return std::rc::Rc::default();
            };
            field
        };
        if fields.len() == DIRECT_CONSTRUCTOR_FIELD_LIMIT
            || fields
                .iter()
                .any(|stored: &crate::facts::DirectConstructorField| stored.name == field.name)
        {
            return std::rc::Rc::default();
        }
        fields.push(field);
    }
    if !fields.is_empty() {
        fields.into()
    } else {
        std::rc::Rc::from([])
    }
}

fn direct_parameter_falsy_fallback(
    statement: &oxc::ast::ast::Statement<'_>,
    parameters: &std::collections::HashMap<String, u16>,
) -> Option<(u16, i32)> {
    use oxc::{
        ast::ast::{AssignmentTarget, Expression, Statement},
        syntax::operator::{AssignmentOperator, UnaryOperator},
    };
    let Statement::IfStatement(branch) = statement else {
        return None;
    };
    branch.alternate.is_none().then_some(())?;
    let Expression::UnaryExpression(test) = &branch.test else {
        return None;
    };
    (test.operator == UnaryOperator::LogicalNot).then_some(())?;
    let Expression::Identifier(tested) = &test.argument else {
        return None;
    };
    let argument = *parameters.get(tested.name.as_str())?;
    let Statement::ExpressionStatement(consequent) = single_statement(&branch.consequent)? else {
        return None;
    };
    let Expression::AssignmentExpression(assignment) = &consequent.expression else {
        return None;
    };
    (assignment.operator == AssignmentOperator::Assign).then_some(())?;
    let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left else {
        return None;
    };
    (target.name == tested.name).then_some(())?;
    let Expression::NumericLiteral(fallback) = &assignment.right else {
        return None;
    };
    Some((argument, exact_i32_literal(fallback.value)?))
}

fn direct_constructor_source_with_fallbacks(
    expression: &oxc::ast::ast::Expression<'_>,
    parameters: &std::collections::HashMap<String, u16>,
    locals: &std::collections::HashMap<String, u16>,
    falsy_fallbacks: &std::collections::HashMap<u16, i32>,
) -> Option<crate::facts::DirectConstructorSource> {
    if let oxc::ast::ast::Expression::Identifier(identifier) = expression {
        let argument = *parameters.get(identifier.name.as_str())?;
        if let Some(fallback) = falsy_fallbacks.get(&argument) {
            return Some(crate::facts::DirectConstructorSource::FalsyArgumentOrInteger {
                argument,
                fallback: *fallback,
            });
        }
    }
    direct_constructor_source(expression, parameters, locals)
}

pub(crate) fn composed_constructor_fact(
    function: &oxc::ast::ast::Function<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> std::rc::Rc<[crate::facts::ComposedConstructorStep]> {
    let Some(body) = function.body.as_ref() else { return std::rc::Rc::default() };
    let Ok((parameters, _)) = crate::function_parameters::bindings(&function.params)
        else { return std::rc::Rc::default() };
    let mut steps = Vec::new();
    let mut has_super = false;
    for statement in &body.statements {
        let step = if let Some((name, expression)) = direct_constructor_assignment(statement) {
            let Some(source) = direct_constructor_source(expression, &parameters, locals)
                else { return std::rc::Rc::default() };
            crate::facts::ComposedConstructorStep::Field(crate::facts::DirectConstructorField {
                name: name.to_string(), source,
            })
        } else if let Some((owner_slot, arguments)) =
            composed_super_call(statement, &parameters, locals)
        {
            has_super = true;
            crate::facts::ComposedConstructorStep::SuperCall { owner_slot, arguments }
        } else {
            return std::rc::Rc::default();
        };
        if steps.len() == 12 { return std::rc::Rc::default(); }
        steps.push(step);
    }
    if has_super { steps.into() } else { std::rc::Rc::default() }
}

fn composed_super_call(
    statement: &oxc::ast::ast::Statement<'_>,
    parameters: &std::collections::HashMap<String, u16>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<(u16, std::rc::Rc<[crate::facts::ForwardValueSource]>)> {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::ExpressionStatement(statement) = statement else { return None };
    let Expression::CallExpression(call) = &statement.expression else { return None };
    let Expression::StaticMemberExpression(invoke) = &call.callee else { return None };
    (invoke.property.name == "call").then_some(())?;
    let Expression::StaticMemberExpression(super_member) = &invoke.object else { return None };
    (super_member.property.name == "superConstructor").then_some(())?;
    let Expression::Identifier(owner) = &super_member.object else { return None };
    let mut arguments = call.arguments.iter();
    matches!(arguments.next()?.as_expression()?, Expression::ThisExpression(_)).then_some(())?;
    let arguments = arguments.map(|argument| direct_constructor_argument(
        argument.as_expression()?, parameters, locals,
    )).collect::<Option<Vec<_>>>()?;
    Some((*locals.get(owner.name.as_str())?, arguments.into()))
}

fn direct_constructor_conditional(
    statement: &oxc::ast::ast::Statement<'_>,
    parameters: &std::collections::HashMap<String, u16>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::DirectConstructorField> {
    use oxc::{
        ast::ast::{Expression, Statement},
        syntax::operator::BinaryOperator,
    };
    let Statement::IfStatement(branch) = statement else {
        return None;
    };
    let Expression::BinaryExpression(test) = &branch.test else {
        return None;
    };
    if test.operator != BinaryOperator::Equality {
        return None;
    }
    let argument = nullish_argument(&test.left, &test.right, parameters)
        .or_else(|| nullish_argument(&test.right, &test.left, parameters))?;
    let (name, nullish) = direct_constructor_assignment(single_statement(&branch.consequent)?)?;
    let (other_name, other) =
        direct_constructor_assignment(single_statement(branch.alternate.as_ref()?)?)?;
    if name != other_name {
        return None;
    }
    Some(crate::facts::DirectConstructorField {
        name: name.to_string(),
        source: crate::facts::DirectConstructorSource::NullishSelectCapture {
            argument,
            nullish_slot: capture_source(nullish, locals)?,
            other_slot: capture_source(other, locals)?,
        },
    })
}

fn single_statement<'a>(
    statement: &'a oxc::ast::ast::Statement<'a>,
) -> Option<&'a oxc::ast::ast::Statement<'a>> {
    let oxc::ast::ast::Statement::BlockStatement(block) = statement else {
        return Some(statement);
    };
    let [statement] = block.body.as_slice() else {
        return None;
    };
    Some(statement)
}

fn nullish_argument(
    value: &oxc::ast::ast::Expression<'_>,
    null: &oxc::ast::ast::Expression<'_>,
    parameters: &std::collections::HashMap<String, u16>,
) -> Option<u16> {
    use oxc::ast::ast::Expression;
    let (Expression::Identifier(value), Expression::NullLiteral(_)) = (value, null) else {
        return None;
    };
    parameters.get(value.name.as_str()).copied()
}

fn capture_source(
    expression: &oxc::ast::ast::Expression<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<u16> {
    let oxc::ast::ast::Expression::Identifier(identifier) = expression else {
        return None;
    };
    locals.get(identifier.name.as_str()).copied()
}

fn direct_constructor_assignment<'a>(
    statement: &'a oxc::ast::ast::Statement<'a>,
) -> Option<(&'a str, &'a oxc::ast::ast::Expression<'a>)> {
    use oxc::{
        ast::ast::{AssignmentTarget, Expression, Statement},
        syntax::operator::AssignmentOperator,
    };
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

fn direct_constructor_source(
    expression: &oxc::ast::ast::Expression<'_>,
    parameters: &std::collections::HashMap<String, u16>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::DirectConstructorSource> {
    use oxc::ast::ast::Expression;
    match expression {
        Expression::Identifier(identifier) => {
            Some(crate::facts::DirectConstructorSource::Argument(
                *parameters.get(identifier.name.as_str())?,
            ))
        }
        Expression::BooleanLiteral(value) => {
            Some(crate::facts::DirectConstructorSource::Boolean(value.value))
        }
        Expression::NullLiteral(_) => Some(crate::facts::DirectConstructorSource::Null),
        Expression::LogicalExpression(logical)
            if logical.operator == oxc::syntax::operator::LogicalOperator::Or =>
        {
            let Expression::Identifier(argument) = &logical.left else { return None };
            let Expression::NumericLiteral(fallback) = &logical.right else { return None };
            let fallback = exact_i32_literal(fallback.value)?;
            Some(crate::facts::DirectConstructorSource::FalsyArgumentOrInteger {
                argument: *parameters.get(argument.name.as_str())?, fallback,
            })
        }
        Expression::NumericLiteral(value)
            if value.value.fract() == 0.0
                && value.value >= i32::MIN as f64
                && value.value <= i32::MAX as f64 =>
        {
            Some(crate::facts::DirectConstructorSource::Integer(
                value.value as i32,
            ))
        }
        Expression::NewExpression(new) => {
            let Expression::Identifier(constructor) = &new.callee else {
                return None;
            };
            if constructor.name == "Array" && !locals.contains_key("Array") {
                return match new.arguments.as_slice() {
                    [] => Some(crate::facts::DirectConstructorSource::EmptyArray),
                    [argument] => {
                        let Expression::Identifier(length) = argument.as_expression()? else {
                            return None;
                        };
                        Some(crate::facts::DirectConstructorSource::GuardedArray {
                            length_slot: *locals.get(length.name.as_str())?,
                        })
                    }
                    _ => None,
                };
            }
            let constructor_slot = *locals.get(constructor.name.as_str())?;
            let arguments = new.arguments.iter().map(|argument| {
                direct_constructor_argument(argument.as_expression()?, parameters, locals)
            }).collect::<Option<Vec<_>>>()?;
            Some(crate::facts::DirectConstructorSource::ConstructCapture {
                constructor_slot, arguments: arguments.into(),
            })
        }
        Expression::StaticMemberExpression(member) => {
            let Expression::Identifier(owner) = &member.object else { return None };
            Some(crate::facts::DirectConstructorSource::CaptureProperty {
                owner_slot: *locals.get(owner.name.as_str())?,
                property: member.property.name.to_string(),
            })
        }
        _ => None,
    }
}

fn exact_i32_literal(value: f64) -> Option<i32> {
    let integer = value as i32;
    (value.is_finite() && value == f64::from(integer)).then_some(integer)
}

fn direct_constructor_argument(
    expression: &oxc::ast::ast::Expression<'_>,
    parameters: &std::collections::HashMap<String, u16>,
    locals: &std::collections::HashMap<String, u16>,
) -> Option<crate::facts::ForwardValueSource> {
    use oxc::ast::ast::Expression;
    match expression {
        Expression::Identifier(identifier) => parameters.get(identifier.name.as_str())
            .copied().map(crate::facts::ForwardValueSource::Argument)
            .or_else(|| locals.get(identifier.name.as_str()).copied()
                .map(crate::facts::ForwardValueSource::Capture)),
        Expression::NumericLiteral(value) => Some(crate::facts::ForwardValueSource::Integer(
            exact_i32_literal(value.value)?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod direct_constructor_tests {
    use super::{composed_constructor_fact, direct_constructor_fact};
    use crate::facts::DirectConstructorSource;
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn fields(source: &str) -> Vec<(String, DirectConstructorSource)> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        direct_constructor_fact(function, &std::collections::HashMap::new())
            .iter()
            .into_iter()
            .map(|field| (field.name.clone(), field.source.clone()))
            .collect()
    }

    #[test]
    fn records_argument_and_boolean_slot_facts() {
        assert_eq!(
            fields("function C(x,y,z){this.x=x;this.y=y;this.ok=false;}"),
            vec![
                ("x".into(), DirectConstructorSource::Argument(0)),
                ("y".into(), DirectConstructorSource::Argument(1)),
                ("ok".into(), DirectConstructorSource::Boolean(false))
            ]
        );
    }

    #[test]
    fn records_prefix_falsy_parameter_normalization_as_field_data() {
        assert_eq!(
            fields(
                "function Vector(x,y,z){if(!x)x=0.0;if(!y)y=0;if(!z)z=0;this.x=x;this.y=y;this.z=z;}"
            ),
            vec![
                (
                    "x".into(),
                    DirectConstructorSource::FalsyArgumentOrInteger {
                        argument: 0,
                        fallback: 0,
                    },
                ),
                (
                    "y".into(),
                    DirectConstructorSource::FalsyArgumentOrInteger {
                        argument: 1,
                        fallback: 0,
                    },
                ),
                (
                    "z".into(),
                    DirectConstructorSource::FalsyArgumentOrInteger {
                        argument: 2,
                        fallback: 0,
                    },
                ),
            ]
        );
    }

    #[test]
    fn rejects_repeated_or_post_field_parameter_normalization() {
        assert!(fields("function C(x){if(!x)x=0;if(!x)x=1;this.x=x;}").is_empty());
        assert!(fields("function C(x){this.x=x;if(!x)x=0;}").is_empty());
    }

    #[test]
    fn records_null_slot_fact() {
        assert_eq!(
            fields("function C(x,y){this.x=x;this.y=y;this.next=null;}"),
            vec![
                ("x".into(), DirectConstructorSource::Argument(0)),
                ("y".into(), DirectConstructorSource::Argument(1)),
                ("next".into(), DirectConstructorSource::Null),
            ]
        );
    }

    #[test]
    fn records_guarded_array_with_resolved_length_slot() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function Packet(a,b,c){this.a=a;this.b=b;this.c=c;this.zero=0;this.data=new Array(SIZE);}",
            SourceType::default(),
        )
        .parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        let locals = std::collections::HashMap::from([("SIZE".to_string(), 9)]);
        let fields = direct_constructor_fact(function, &locals);
        assert_eq!(fields[3].source, DirectConstructorSource::Integer(0));
        assert_eq!(
            fields[4].source,
            DirectConstructorSource::GuardedArray { length_slot: 9 }
        );
    }

    #[test]
    fn records_nullish_capture_selection() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function T(a,b,c){this.a=a;this.b=b;this.c=c;if(b==null){this.state=EMPTY;}else{this.state=READY;}}",
            SourceType::default(),
        )
        .parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        let locals =
            std::collections::HashMap::from([("EMPTY".to_string(), 4), ("READY".to_string(), 5)]);
        let fields = direct_constructor_fact(function, &locals);
        assert_eq!(
            fields[3].source,
            DirectConstructorSource::NullishSelectCapture {
                argument: 1,
                nullish_slot: 4,
                other_slot: 5,
            }
        );
    }

    #[test]
    fn records_composed_record_sources() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function V(name,value){this.value=value||0;this.items=new C();this.rank=S.WEAKEST;this.name=name;}",
            SourceType::default(),
        ).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else { panic!() };
        let locals = std::collections::HashMap::from([
            ("C".to_string(), 7), ("S".to_string(), 8),
        ]);
        let fields = direct_constructor_fact(function, &locals);
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].source,
            DirectConstructorSource::FalsyArgumentOrInteger { argument: 1, fallback: 0 });
        assert!(matches!(fields[1].source,
            DirectConstructorSource::ConstructCapture { constructor_slot: 7, ref arguments }
                if arguments.is_empty()));
        assert_eq!(fields[2].source, DirectConstructorSource::CaptureProperty {
            owner_slot: 8, property: "WEAKEST".into(),
        });
    }

    #[test]
    fn records_super_calls_as_ordered_constructor_steps() {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            "function Child(x,y){this.before=1;Child.superConstructor.call(this,x,y);this.after=x;}",
            SourceType::default(),
        ).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else { panic!() };
        let steps = composed_constructor_fact(function,
            &std::collections::HashMap::from([("Child".to_string(), 4)]));
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[1], crate::facts::ComposedConstructorStep::SuperCall {
            owner_slot: 4, ref arguments,
        } if arguments.as_ref() == [
            crate::facts::ForwardValueSource::Argument(0),
            crate::facts::ForwardValueSource::Argument(1),
        ]));
    }

    #[test]
    fn rejects_receiver_dependent_or_conditional_initialization() {
        assert!(fields("function C(x,y,z){this.x=x;this.y=this.x;this.z=z;}").is_empty());
        assert!(fields("function C(x,y,z){if(x)this.x=x;this.y=y;this.z=z;}").is_empty());
    }

    #[test]
    fn declaration_carries_one_immutable_function_fact() {
        let program = crate::reduce::reduce_global_script_source(
            "function C(x,y,z){this.x=x;this.y=y;this.z=z;}",
        )
        .expect("function declaration reduces");
        let code = program.code();
        let facts = (0..code.len()).find_map(|pc| {
            let instruction = code.instruction(pc)?;
            let crate::ops::Op::MakeFunctionWithKind { body, .. } = code.cold(instruction)? else {
                return None;
            };
            Some(body.facts().direct_constructor.clone())
        });
        assert_eq!(facts.expect("function body fact").len(), 3);
    }
}
