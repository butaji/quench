const DIRECT_CONSTRUCTOR_FIELD_LIMIT: usize = 8;
const DIRECT_FALSE: i16 = -1;
const DIRECT_TRUE: i16 = -2;

pub(crate) fn direct_constructor_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Vec<DirectConstructorField> {
    let Some(body) = function.body.as_ref() else {
        return Vec::new();
    };
    let Ok((parameters, _)) = crate::function_parameters::bindings(&function.params) else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    for statement in &body.statements {
        let Some((name, expression)) = direct_constructor_assignment(statement) else {
            return Vec::new();
        };
        if fields.len() == DIRECT_CONSTRUCTOR_FIELD_LIMIT
            || fields
                .iter()
                .any(|field: &DirectConstructorField| field.name == name)
        {
            return Vec::new();
        }
        let Some(source) = direct_constructor_source(expression, &parameters) else {
            return Vec::new();
        };
        fields.push(DirectConstructorField {
            name: name.to_string(),
            source,
        });
    }
    (fields.len() >= 3).then_some(fields).unwrap_or_default()
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
) -> Option<i16> {
    use oxc::ast::ast::Expression;
    match expression {
        Expression::Identifier(identifier) => {
            i16::try_from(*parameters.get(identifier.name.as_str())?).ok()
        }
        Expression::BooleanLiteral(value) => Some(if value.value {
            DIRECT_TRUE
        } else {
            DIRECT_FALSE
        }),
        _ => None,
    }
}

#[cfg(test)]
mod direct_constructor_tests {
    use super::{direct_constructor_fact, DIRECT_FALSE};
    use oxc::{allocator::Allocator, ast::ast::Statement, parser::Parser, span::SourceType};

    fn fields(source: &str) -> Vec<(String, i16)> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
            panic!("expected function declaration");
        };
        direct_constructor_fact(function)
            .into_iter()
            .map(|field| (field.name, field.source))
            .collect()
    }

    #[test]
    fn records_argument_and_boolean_slot_facts() {
        assert_eq!(
            fields("function C(x,y,z){this.x=x;this.y=y;this.ok=false;}"),
            vec![
                ("x".into(), 0),
                ("y".into(), 1),
                ("ok".into(), DIRECT_FALSE)
            ]
        );
    }

    #[test]
    fn rejects_receiver_dependent_or_conditional_initialization() {
        assert!(fields("function C(x,y,z){this.x=x;this.y=this.x;this.z=z;}").is_empty());
        assert!(fields("function C(x,y,z){if(x)this.x=x;this.y=y;this.z=z;}").is_empty());
    }
}
