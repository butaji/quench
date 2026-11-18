//! TypeScript `enum` declarations, compiled as the runtime object TypeScript defines for them.
//!
//! An enum is a binding to an object holding each member's value under its name and, for a
//! numeric member, the name under its value: `enum E { A, B = "b" }` runs as
//! `var E; E || (E = {}), E[E["A"] = 0] = "A", E["B"] = "b"`. The tree is rewritten to that
//! form right after parsing, so every analysis of the program (bindings, hoisting, early
//! errors, replay) reads one form. No function is synthesized, so a replay of any function
//! of the program parses and rewrites the same text alike.
//!
//! Member values fold as TypeScript folds them: literals, an earlier member by name, unary and
//! binary arithmetic over those. A member without an initializer follows a numeric one by one.
//! Any other initializer runs where the enum runs, with the enum's members named through the
//! enum (`B = f(A)` runs `f(E.A)`). A `declare enum` has no runtime part; a `const enum` keeps
//! its object (TypeScript's `preserveConstEnums`), which its inlined uses would equal.

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_ast::builder::AstBuilder;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_allocator::CloneIn;
use oxc_span::Span;
use oxc_syntax::number::NumberBase;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};
use rustc_hash::FxHashMap;

/// Rewrites every runtime `enum` of `program` to its object form; the span and message of an
/// enum TypeScript rejects too (a member without initializer after a computed one).
pub(super) fn lower<'a>(allocator: &'a Allocator, program: &mut Program<'a>) -> Result<(), (Span, &'static str)> {
    let mut lowering = Lowering { allocator, error: None, block_depth: 0 };
    lowering.visit_program(program);
    lowering.error.map_or(Ok(()), Err)
}

#[derive(Clone, Copy)]
enum Value<'a> {
    Number(f64),
    String(&'a str),
}

struct Lowering<'a> {
    allocator: &'a Allocator,
    error: Option<(Span, &'static str)>,
    /// Statement lists entered that are blocks, not function or program bodies: an enum there
    /// binds with `let`, as TypeScript emits it.
    block_depth: u32,
}

impl<'a> VisitMut<'a> for Lowering<'a> {
    fn visit_statements(&mut self, statements: &mut ArenaVec<'a, Statement<'a>>) {
        if self.error.is_none() && statements.iter().any(holds_runtime_enum) {
            self.lower_list(statements);
        }
        walk_mut::walk_statements(self, statements);
    }

    fn visit_block_statement(&mut self, block: &mut BlockStatement<'a>) {
        self.block_depth += 1;
        walk_mut::walk_block_statement(self, block);
        self.block_depth -= 1;
    }

    fn visit_switch_case(&mut self, case: &mut SwitchCase<'a>) {
        self.block_depth += 1;
        walk_mut::walk_switch_case(self, case);
        self.block_depth -= 1;
    }

    fn visit_function_body(&mut self, body: &mut FunctionBody<'a>) {
        let depth = std::mem::take(&mut self.block_depth);
        walk_mut::walk_function_body(self, body);
        self.block_depth = depth;
    }

    fn visit_static_block(&mut self, block: &mut StaticBlock<'a>) {
        let depth = std::mem::take(&mut self.block_depth);
        walk_mut::walk_static_block(self, block);
        self.block_depth = depth;
    }
}

fn holds_runtime_enum(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::TSEnumDeclaration(declaration) => !declaration.declare,
        Statement::ExportDeclaration(export) => {
            matches!(&export.declaration, Declaration::TSEnumDeclaration(declaration) if !declaration.declare)
        }
        _ => false,
    }
}

impl<'a> Lowering<'a> {
    fn lower_list(&mut self, statements: &mut ArenaVec<'a, Statement<'a>>) {
        let builder = AstBuilder::new(self.allocator);
        let old = std::mem::replace(statements, ArenaVec::new_in(&self.allocator));
        // The enums of this list already bound: a later declaration merges into the object.
        let mut bound: Vec<&'a str> = Vec::new();
        for statement in old {
            match statement {
                Statement::TSEnumDeclaration(declaration) if declaration.declare => {}
                Statement::TSEnumDeclaration(declaration) => {
                    let name = declaration.id.name.as_str();
                    if !bound.contains(&name) {
                        bound.push(name);
                        statements.push(Statement::VariableDeclaration(self.binding(&declaration, &builder)));
                    }
                    match self.members(&declaration, &builder) {
                        Some(statement) => statements.push(statement),
                        None => return,
                    }
                }
                Statement::ExportDeclaration(mut export)
                    if matches!(&export.declaration, Declaration::TSEnumDeclaration(declaration) if !declaration.declare) =>
                {
                    let Declaration::TSEnumDeclaration(declaration) = &export.declaration else { unreachable!() };
                    let name = declaration.id.name.as_str();
                    let members = self.members(declaration, &builder);
                    let binding = (!bound.contains(&name)).then(|| self.binding(declaration, &builder));
                    if let Some(binding) = binding {
                        bound.push(name);
                        export.declaration = Declaration::VariableDeclaration(binding);
                        statements.push(Statement::ExportDeclaration(export));
                    }
                    match members {
                        Some(statement) => statements.push(statement),
                        None => return,
                    }
                }
                statement => statements.push(statement),
            }
        }
    }

    /// `var E` (`let E` in a block), the enum's binding.
    fn binding(&self, declaration: &TSEnumDeclaration<'a>, builder: &AstBuilder<'a>) -> oxc_allocator::Box<'a, VariableDeclaration<'a>> {
        let id = &declaration.id;
        let pattern = BindingPattern::new_binding_identifier(id.span, id.name, builder);
        let declarator = VariableDeclarator::new(id.span, pattern, None, None, false, builder);
        let kind = if self.block_depth > 0 { VariableDeclarationKind::Let } else { VariableDeclarationKind::Var };
        oxc_allocator::Box::new_in(
            VariableDeclaration::new(declaration.span, kind, ArenaVec::from_iter_in([declarator], &self.allocator), false, builder),
            &self.allocator,
        )
    }

    /// `E || (E = {}), E[E["A"] = 0] = "A", …`: the object and each member, in order.
    fn members(&mut self, declaration: &TSEnumDeclaration<'a>, builder: &AstBuilder<'a>) -> Option<Statement<'a>> {
        let span = declaration.span;
        let name = declaration.id.name;
        let enum_ref = |span: Span| Expression::new_identifier(span, name, builder);
        let assign_enum = AssignmentTarget::new_assignment_target_identifier(span, name, builder);
        let empty = Expression::new_object_expression(span, ArenaVec::new_in(&self.allocator), builder);
        let made = Expression::new_assignment_expression(span, AssignmentOperator::Assign, assign_enum, empty, builder);
        let made = Expression::new_parenthesized_expression(span, made, builder);
        let mut expressions = ArenaVec::new_in(&self.allocator);
        expressions.push(Expression::new_logical_expression(span, enum_ref(span), LogicalOperator::Or, made, builder));
        let names: Vec<&'a str> = declaration.body.members.iter().filter_map(|member| self.member_name(&member.id)).collect();
        let mut known: FxHashMap<&'a str, Value<'a>> = FxHashMap::default();
        // The last member's value when it was a number TypeScript folded; `Some(-1.0)` first.
        let mut previous = Some(-1.0);
        for member in &declaration.body.members {
            let span = member.span;
            let Some(key) = self.member_name(&member.id) else {
                self.error = Some((span, "a computed enum member name needs a literal string"));
                return None;
            };
            let (value, folded) = match &member.initializer {
                Some(initializer) => match fold(initializer, &known, name.as_str()) {
                    Some(value) => (self.literal(span, value, builder), Some(value)),
                    None => {
                        let mut expression = initializer.clone_in(self.allocator);
                        MemberReferences { allocator: self.allocator, enum_name: name.as_str(), names: &names }.visit_expression(&mut expression);
                        (expression, None)
                    }
                },
                None => match previous {
                    Some(number) => {
                        let value = Value::Number(number + 1.0);
                        (self.literal(span, value, builder), Some(value))
                    }
                    None => {
                        self.error = Some((span, "an enum member after a computed one needs an initializer"));
                        return None;
                    }
                },
            };
            previous = match folded {
                Some(Value::Number(number)) => Some(number),
                _ => None,
            };
            if let Some(value) = folded {
                known.insert(key, value);
            }
            let key_literal = || Expression::new_string_literal(span, key, None, builder);
            let slot = AssignmentTarget::new_computed_member_expression(span, enum_ref(span), key_literal(), false, builder);
            let store = Expression::new_assignment_expression(span, AssignmentOperator::Assign, slot, value, builder);
            // A string member has no reverse entry; every other one names itself by its value.
            let reverse = !matches!(folded, Some(Value::String(_))) && !is_string_literal(member.initializer.as_ref());
            expressions.push(if reverse {
                let slot = AssignmentTarget::new_computed_member_expression(span, enum_ref(span), store, false, builder);
                Expression::new_assignment_expression(span, AssignmentOperator::Assign, slot, key_literal(), builder)
            } else {
                store
            });
        }
        let sequence = Expression::new_sequence_expression(span, expressions, builder);
        Some(Statement::new_expression_statement(span, sequence, builder))
    }

    fn member_name(&self, name: &TSEnumMemberName<'a>) -> Option<&'a str> {
        match name {
            TSEnumMemberName::Identifier(identifier) => Some(identifier.name.as_str()),
            TSEnumMemberName::String(literal) | TSEnumMemberName::ComputedString(literal) => Some(literal.value.as_str()),
            TSEnumMemberName::ComputedTemplateString(template) => {
                template.single_quasi().map(|quasi| quasi.as_str())
            }
        }
    }

    fn literal(&self, span: Span, value: Value<'a>, builder: &AstBuilder<'a>) -> Expression<'a> {
        match value {
            Value::Number(number) if number.is_sign_negative() && number != 0.0 || number.is_nan() || number.is_infinite() => {
                // A literal is never negative or not finite: write those as arithmetic.
                if number.is_nan() {
                    let zero = Expression::new_numeric_literal(span, 0.0, None, NumberBase::Decimal, builder);
                    let zero2 = Expression::new_numeric_literal(span, 0.0, None, NumberBase::Decimal, builder);
                    Expression::new_binary_expression(span, zero, BinaryOperator::Division, zero2, builder)
                } else if number.is_infinite() {
                    let one = Expression::new_numeric_literal(span, number.signum(), None, NumberBase::Decimal, builder);
                    let one = if number < 0.0 {
                        let positive = Expression::new_numeric_literal(span, 1.0, None, NumberBase::Decimal, builder);
                        Expression::new_unary_expression(span, UnaryOperator::UnaryNegation, positive, builder)
                    } else {
                        one
                    };
                    let zero = Expression::new_numeric_literal(span, 0.0, None, NumberBase::Decimal, builder);
                    Expression::new_binary_expression(span, one, BinaryOperator::Division, zero, builder)
                } else {
                    let magnitude = Expression::new_numeric_literal(span, -number, None, NumberBase::Decimal, builder);
                    Expression::new_unary_expression(span, UnaryOperator::UnaryNegation, magnitude, builder)
                }
            }
            Value::Number(number) => Expression::new_numeric_literal(span, number, None, NumberBase::Decimal, builder),
            Value::String(text) => Expression::new_string_literal(span, text, None, builder),
        }
    }
}

fn is_string_literal(initializer: Option<&Expression<'_>>) -> bool {
    matches!(initializer.map(Expression::without_parentheses), Some(Expression::StringLiteral(_) | Expression::TemplateLiteral(_)))
}

/// The value TypeScript folds `expression` to, given the members folded so far.
fn fold<'a>(expression: &Expression<'a>, known: &FxHashMap<&'a str, Value<'a>>, enum_name: &str) -> Option<Value<'a>> {
    Some(match expression {
        Expression::NumericLiteral(literal) => Value::Number(literal.value),
        Expression::StringLiteral(literal) => Value::String(literal.value.as_str()),
        Expression::TemplateLiteral(template) => Value::String(template.single_quasi()?.as_str()),
        Expression::ParenthesizedExpression(inner) => fold(&inner.expression, known, enum_name)?,
        Expression::Identifier(identifier) => *known.get(identifier.name.as_str())?,
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) if object.name == enum_name => *known.get(member.property.name.as_str())?,
            _ => return None,
        },
        Expression::ComputedMemberExpression(member) => match (&member.object, &member.expression) {
            (Expression::Identifier(object), Expression::StringLiteral(key)) if object.name == enum_name => {
                *known.get(key.value.as_str())?
            }
            _ => return None,
        },
        Expression::UnaryExpression(unary) => {
            let Value::Number(operand) = fold(&unary.argument, known, enum_name)? else { return None };
            Value::Number(match unary.operator {
                UnaryOperator::UnaryPlus => operand,
                UnaryOperator::UnaryNegation => -operand,
                UnaryOperator::BitwiseNot => f64::from(!to_int32(operand)),
                _ => return None,
            })
        }
        Expression::BinaryExpression(binary) => {
            let left = fold(&binary.left, known, enum_name)?;
            let right = fold(&binary.right, known, enum_name)?;
            match (left, right, binary.operator) {
                (Value::Number(left), Value::Number(right), operator) => Value::Number(number_operation(operator, left, right)?),
                (Value::String(_), _, BinaryOperator::Addition) | (_, Value::String(_), BinaryOperator::Addition) => {
                    // String folding needs an arena string; leave it to run.
                    return None;
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn number_operation(operator: BinaryOperator, left: f64, right: f64) -> Option<f64> {
    Some(match operator {
        BinaryOperator::Addition => left + right,
        BinaryOperator::Subtraction => left - right,
        BinaryOperator::Multiplication => left * right,
        BinaryOperator::Division => left / right,
        BinaryOperator::Remainder => left % right,
        BinaryOperator::Exponential => left.powf(right),
        BinaryOperator::BitwiseOR => f64::from(to_int32(left) | to_int32(right)),
        BinaryOperator::BitwiseAnd => f64::from(to_int32(left) & to_int32(right)),
        BinaryOperator::BitwiseXOR => f64::from(to_int32(left) ^ to_int32(right)),
        BinaryOperator::ShiftLeft => f64::from(to_int32(left).wrapping_shl(to_uint32(right) & 31)),
        BinaryOperator::ShiftRight => f64::from(to_int32(left).wrapping_shr(to_uint32(right) & 31)),
        BinaryOperator::ShiftRightZeroFill => f64::from(to_uint32(left).wrapping_shr(to_uint32(right) & 31)),
        _ => return None,
    })
}

/// ECMA-262 ToInt32 of a Number.
fn to_int32(value: f64) -> i32 {
    to_uint32(value) as i32
}

/// ECMA-262 ToUint32 of a Number.
fn to_uint32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    (value.trunc().rem_euclid(4_294_967_296.0)) as u32
}

/// Names an enum's members through the enum in an initializer that runs: `f(A)` as `f(E.A)`.
struct MemberReferences<'s, 'a> {
    allocator: &'a Allocator,
    enum_name: &'a str,
    names: &'s [&'a str],
}

impl<'a> VisitMut<'a> for MemberReferences<'_, 'a> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Expression::Identifier(identifier) = expression
            && self.names.contains(&identifier.name.as_str())
        {
            let builder = AstBuilder::new(self.allocator);
            let span = identifier.span;
            let property = IdentifierName::new(span, identifier.name.as_str(), &builder);
            let object = Expression::new_identifier(span, self.enum_name, &builder);
            *expression = Expression::new_static_member_expression(span, object, property, false, &builder);
            return;
        }
        walk_mut::walk_expression(self, expression);
    }

    // A nested function's own bindings may shadow a member name; TypeScript names the member
    // only where it resolves, which a function boundary is the simple, safe limit of.
    fn visit_function(&mut self, _: &mut Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &mut ArrowFunctionExpression<'a>) {}
}
