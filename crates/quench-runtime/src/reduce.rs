//! OXC-to-residual reduction entry point.

use std::collections::HashMap;

use oxc::{
    allocator::Allocator,
    ast::ast::{BindingPatternKind, Expression, Statement},
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    syntax::operator::BinaryOperator,
};

use crate::{
    facts::ProgramDb,
    ops::{Constant, Op},
};

#[derive(Debug, PartialEq)]
pub struct ResidualProgram {
    pub facts: ProgramDb,
    pub ops: Vec<Op>,
}

pub fn reduce_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
    if parsed.panicked {
        return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
    }
    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|error| format!("SyntaxError: {error}"))
            .collect());
    }
    let (scope_count, symbol_count) = analyze_semantics(&parsed.program)?;
    let mut facts = ProgramDb {
        scope_count,
        symbol_count,
        ..ProgramDb::default()
    };
    let ops = reduce_statements(&parsed.program.body, &mut facts)?;
    Ok(ResidualProgram { facts, ops })
}

fn reduce_statements(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops = Vec::new();
    let mut next_register = 0;
    let mut next_slot = 0;
    let mut locals = HashMap::new();
    for statement in statements {
        match statement {
            Statement::EmptyStatement(_) => {}
            Statement::VariableDeclaration(declaration) => reduce_declaration(
                declaration,
                &mut ops,
                facts,
                &mut next_register,
                &mut next_slot,
                &mut locals,
            )?,
            Statement::ExpressionStatement(expression) => reduce_expression_statement(
                &expression.expression,
                &mut ops,
                facts,
                &mut next_register,
                &locals,
            )?,
            _ => return Err(vec!["Unsupported executable statement".to_string()]),
        }
    }
    if ops.is_empty() {
        ops.push(Op::Const {
            dst: 0,
            value: Constant::Undefined,
        });
        ops.push(Op::Return { src: 0 });
    }
    Ok(ops)
}

fn reduce_expression_statement(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let Some(register) = reduce_expression(expression, ops, facts, next_register, locals) else {
        return Err(vec!["Unsupported executable expression".to_string()]);
    };
    ops.push(Op::Return { src: register });
    Ok(())
}

fn reduce_declaration(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    for declarator in &declaration.declarations {
        let BindingPatternKind::BindingIdentifier(identifier) = &declarator.id.kind else {
            return Err(vec!["Unsupported binding pattern".to_string()]);
        };
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(identifier.name.to_string(), slot);
        let register = match declarator.init.as_ref() {
            Some(init) => reduce_expression(init, ops, facts, next_register, locals),
            None => Some(emit_undefined(ops, next_register)),
        };
        let Some(register) = register else {
            return Err(vec!["Unsupported variable initializer".to_string()]);
        };
        ops.push(Op::StoreLocal {
            slot,
            src: register,
        });
    }
    Ok(())
}

fn emit_undefined(ops: &mut Vec<Op>, next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: register,
        value: Constant::Undefined,
    });
    register
}

fn reduce_expression(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(register) = reduce_atom(expression, ops, facts, next_register, locals) {
        return Some(register);
    }
    let Expression::BinaryExpression(binary) = expression else {
        return None;
    };
    let operator = reduce_operator(binary.operator)?;
    let lhs = reduce_expression(&binary.left, ops, facts, next_register, locals)?;
    let rhs = reduce_expression(&binary.right, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Binary {
        dst,
        operator,
        lhs,
        rhs,
    });
    Some(dst)
}

fn reduce_atom(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(value) = reduce_literal(expression) {
        let register = *next_register;
        *next_register = next_register.saturating_add(1);
        facts.constants.push(crate::facts::ConstantFact {
            value: value.fact.clone(),
        });
        ops.push(Op::Const {
            dst: register,
            value: value.op,
        });
        return Some(register);
    }
    if let Expression::Identifier(identifier) = expression {
        let slot = *locals.get(identifier.name.as_str())?;
        let register = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::LoadLocal {
            dst: register,
            slot,
        });
        return Some(register);
    }
    None
}

struct Literal {
    fact: crate::facts::Constant,
    op: Constant,
}

fn reduce_literal(expression: &Expression<'_>) -> Option<Literal> {
    match expression {
        Expression::NumericLiteral(number) => Some(Literal {
            fact: crate::facts::Constant::Number(number.value),
            op: Constant::Number(number.value),
        }),
        Expression::BooleanLiteral(boolean) => Some(Literal {
            fact: crate::facts::Constant::Boolean(boolean.value),
            op: Constant::Boolean(boolean.value),
        }),
        Expression::StringLiteral(string) => Some(Literal {
            fact: crate::facts::Constant::String(string.value.to_string()),
            op: Constant::String(string.value.to_string()),
        }),
        Expression::NullLiteral(_) => Some(Literal {
            fact: crate::facts::Constant::Null,
            op: Constant::Null,
        }),
        _ => None,
    }
}

fn reduce_operator(operator: BinaryOperator) -> Option<crate::ops::BinaryOp> {
    Some(match operator {
        BinaryOperator::Addition => crate::ops::BinaryOp::Add,
        BinaryOperator::Subtraction => crate::ops::BinaryOp::Subtract,
        BinaryOperator::Multiplication => crate::ops::BinaryOp::Multiply,
        BinaryOperator::Division => crate::ops::BinaryOp::Divide,
        BinaryOperator::Remainder => crate::ops::BinaryOp::Remainder,
        BinaryOperator::Exponential => crate::ops::BinaryOp::Exponentiate,
        _ => return None,
    })
}

fn analyze_semantics(program: &oxc::ast::ast::Program<'_>) -> Result<(usize, usize), Vec<String>> {
    let semantic = SemanticBuilder::new().build(program);
    if !semantic.errors.is_empty() {
        return Err(semantic
            .errors
            .iter()
            .map(|error| format!("SyntaxError: {error}"))
            .collect());
    }
    Ok((
        semantic.semantic.scopes().len(),
        semantic.semantic.symbols().len(),
    ))
}
