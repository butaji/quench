//! OXC-to-residual reduction entry point.

use oxc::{
    allocator::Allocator,
    ast::ast::{Expression, Statement},
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
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
    for statement in parsed.program.body.iter() {
        let Statement::ExpressionStatement(expression) = statement else {
            continue;
        };
        let Expression::NumericLiteral(number) = &expression.expression else {
            continue;
        };
        let value = number.value;
        facts.constants.push(crate::facts::ConstantFact {
            value: crate::facts::Constant::Number(value),
        });
        ops.push(Op::Const {
            dst: 0,
            value: Constant::Number(value),
        });
        ops.push(Op::Return { src: 0 });
    }
    Ok(ResidualProgram { facts, ops })
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
