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
    for statement in &body.statements {
        let field = if let Some((name, expression)) = direct_constructor_assignment(statement) {
            let Some(source) = direct_constructor_source(expression, &parameters, locals) else {
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
    if fields.len() >= 3 {
        fields.into()
    } else {
        std::rc::Rc::from([])
    }
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
            let [argument] = new.arguments.as_slice() else {
                return None;
            };
            let Expression::Identifier(length) = argument.as_expression()? else {
                return None;
            };
            (constructor.name == "Array" && !locals.contains_key("Array")).then_some(
                crate::facts::DirectConstructorSource::GuardedArray {
                    length_slot: *locals.get(length.name.as_str())?,
                },
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod direct_constructor_tests {
    use super::direct_constructor_fact;
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
