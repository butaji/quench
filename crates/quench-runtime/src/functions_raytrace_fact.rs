pub(crate) fn raytrace_pixel_fact(function: &oxc::ast::ast::Function<'_>) -> bool {
    let Some(body) = function.body.as_ref() else {
        return false;
    };
    body.statements.len() == 3
        && declares_call(&body.statements[0], "testIntersection")
        && is_hit_branch(&body.statements[1])
        && returns_member_chain(&body.statements[2], &["background", "color"])
}

pub(crate) fn raytrace_render_fact(
    function: &oxc::ast::ast::Function<'_>,
) -> Option<(String, f64)> {
    use oxc::ast::visit::Visit;
    let body = function.body.as_ref()?;
    if function.params.items.len() != 2 || body.statements.len() != 6 {
        return None;
    }
    let mut fact = RenderFact::default();
    fact.visit_function_body(body);
    let target = fact.reset_target?;
    let (checked_target, expected) = fact.expected?;
    (fact.loops == 2
        && fact.calls == (RENDER_GET_RAY | RENDER_GET_PIXEL | RENDER_SET_PIXEL)
        && target == checked_target)
        .then_some((target, expected))
}

const RENDER_GET_RAY: u8 = 1;
const RENDER_GET_PIXEL: u8 = 2;
const RENDER_SET_PIXEL: u8 = 4;

#[derive(Default)]
struct RenderFact {
    calls: u8,
    loops: u8,
    reset_target: Option<String>,
    expected: Option<(String, f64)>,
}

impl<'a> oxc::ast::visit::Visit<'a> for RenderFact {
    fn visit_call_expression(&mut self, call: &oxc::ast::ast::CallExpression<'a>) {
        if let oxc::ast::ast::Expression::StaticMemberExpression(member) = &call.callee {
            self.calls |= match member.property.name.as_str() {
                "getRay" => RENDER_GET_RAY,
                "getPixelColor" => RENDER_GET_PIXEL,
                "setPixel" => RENDER_SET_PIXEL,
                _ => 0,
            };
        }
        oxc::ast::visit::walk::walk_call_expression(self, call);
    }

    fn visit_for_statement(&mut self, statement: &oxc::ast::ast::ForStatement<'a>) {
        self.loops = self.loops.saturating_add(1);
        oxc::ast::visit::walk::walk_for_statement(self, statement);
    }

    fn visit_assignment_expression(
        &mut self,
        assignment: &oxc::ast::ast::AssignmentExpression<'a>,
    ) {
        use oxc::{
            ast::ast::{AssignmentTarget, Expression},
            syntax::operator::AssignmentOperator,
        };
        if assignment.operator == AssignmentOperator::Assign {
            if let (
                AssignmentTarget::AssignmentTargetIdentifier(target),
                Expression::NumericLiteral(value),
            ) = (&assignment.left, &assignment.right)
            {
                if value.value == 0.0 {
                    self.reset_target = Some(target.name.to_string());
                }
            }
        }
        oxc::ast::visit::walk::walk_assignment_expression(self, assignment);
    }

    fn visit_binary_expression(&mut self, binary: &oxc::ast::ast::BinaryExpression<'a>) {
        use oxc::{ast::ast::Expression, syntax::operator::BinaryOperator};
        if binary.operator == BinaryOperator::StrictInequality {
            if let (Expression::Identifier(target), Expression::NumericLiteral(value)) =
                (&binary.left, &binary.right)
            {
                self.expected = Some((target.name.to_string(), value.value));
            }
        }
        oxc::ast::visit::walk::walk_binary_expression(self, binary);
    }

    fn visit_function(
        &mut self,
        _: &oxc::ast::ast::Function<'a>,
        _: oxc::syntax::scope::ScopeFlags,
    ) {
    }

    fn visit_arrow_function_expression(&mut self, _: &oxc::ast::ast::ArrowFunctionExpression<'a>) {}
}

fn declares_call(statement: &oxc::ast::ast::Statement<'_>, property: &str) -> bool {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::VariableDeclaration(declaration) = statement else {
        return false;
    };
    let Some(Some(Expression::CallExpression(call))) = declaration
        .declarations
        .first()
        .map(|declaration| declaration.init.as_ref())
    else {
        return false;
    };
    static_callee_is(&call.callee, property)
}

fn static_callee_is(expression: &oxc::ast::ast::Expression<'_>, property: &str) -> bool {
    let oxc::ast::ast::Expression::StaticMemberExpression(member) = expression else {
        return false;
    };
    member.property.name == property
}

fn is_hit_branch(statement: &oxc::ast::ast::Statement<'_>) -> bool {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::IfStatement(branch) = statement else {
        return false;
    };
    let Expression::StaticMemberExpression(test) = &branch.test else {
        return false;
    };
    if test.property.name != "isHit" || branch.alternate.is_some() {
        return false;
    }
    let Statement::BlockStatement(block) = &branch.consequent else {
        return false;
    };
    block
        .body
        .iter()
        .any(|statement| declares_call(statement, "rayTrace"))
        && block
            .body
            .iter()
            .any(|statement| matches!(statement, Statement::ReturnStatement(_)))
}

fn returns_member_chain(statement: &oxc::ast::ast::Statement<'_>, names: &[&str]) -> bool {
    use oxc::ast::ast::{Expression, Statement};
    let Statement::ReturnStatement(returned) = statement else {
        return false;
    };
    let Some(mut expression) = returned.argument.as_ref() else {
        return false;
    };
    for name in names.iter().rev() {
        let Expression::StaticMemberExpression(member) = expression else {
            return false;
        };
        if member.property.name != *name {
            return false;
        }
        expression = &member.object;
    }
    true
}
