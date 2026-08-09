//! OXC-to-residual reduction entry point.

use oxc::{
    allocator::Allocator,
    ast::ast::{Expression, Statement},
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
    let mut ops = Vec::new();
    let mut next_register = 0;
    for statement in parsed.program.body.iter() {
        let Statement::ExpressionStatement(expression) = statement else {
            continue;
        };
        let Some(register) = reduce_expression(
            &expression.expression,
            &mut ops,
            &mut facts,
            &mut next_register,
        ) else {
            continue;
        };
        ops.push(Op::Return { src: register });
    }
    Ok(ResidualProgram { facts, ops })
}

fn reduce_expression(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
) -> Option<u16> {
    if let Expression::NumericLiteral(number) = expression {
        let register = *next_register;
        *next_register = next_register.saturating_add(1);
        facts.constants.push(crate::facts::ConstantFact {
            value: crate::facts::Constant::Number(number.value),
        });
        ops.push(Op::Const {
            dst: register,
            value: Constant::Number(number.value),
        });
        return Some(register);
    }
    let Expression::BinaryExpression(binary) = expression else {
        return None;
    };
    let operator = reduce_operator(binary.operator)?;
    let lhs = reduce_expression(&binary.left, ops, facts, next_register)?;
    let rhs = reduce_expression(&binary.right, ops, facts, next_register)?;
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
