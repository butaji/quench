//! Lower helpers - shared utilities for OXC AST lowering

use crate::ast::{BinaryOp, CompoundOp, UnaryOp};
use oxc::ast::ast;
use oxc::syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};

/// LowerError during lowering
#[derive(Debug, Clone)]
pub struct LowerError {
    pub message: String,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LowerError {}

impl LowerError {
    pub fn new(message: impl Into<String>) -> Self {
        LowerError {
            message: message.into(),
        }
    }
}

/// Lower an OXC function body, preserving its directive prologue.
///
/// OXC stores directives (e.g. `"use strict"`) in `body.directives`, separate
/// from `body.statements`. We prepend them as string expression statements so
/// the interpreter's strict-mode detection (check_use_strict) can find them.
pub fn lower_fn_body(body: &ast::FunctionBody) -> Vec<crate::ast::Statement> {
    let mut stmts: Vec<crate::ast::Statement> = body
        .directives
        .iter()
        .map(|d| {
            crate::ast::Statement::Expression(Box::new(crate::ast::Expression::String(
                d.expression.value.to_string(),
            )))
        })
        .collect();
    stmts.extend(body.statements.iter().filter_map(super::stmt::lower_stmt));
    stmts
}

/// Convert Wtf8Atom to String using Display trait
pub fn wtf8_atom_to_string(atom: &str) -> String {
    atom.to_string()
}

/// Lower binary operator
#[allow(clippy::complexity)]
pub fn lower_bin_op(op: &BinaryOperator) -> Result<BinaryOp, LowerError> {
    match op {
        BinaryOperator::Addition => Ok(BinaryOp::Add),
        BinaryOperator::Subtraction => Ok(BinaryOp::Sub),
        BinaryOperator::Multiplication => Ok(BinaryOp::Mul),
        BinaryOperator::Division => Ok(BinaryOp::Div),
        BinaryOperator::Remainder => Ok(BinaryOp::Mod),
        BinaryOperator::Exponential => Ok(BinaryOp::Mul), // approximation
        BinaryOperator::ShiftLeft => Ok(BinaryOp::Shl),
        BinaryOperator::ShiftRight => Ok(BinaryOp::Shr),
        BinaryOperator::ShiftRightZeroFill => Ok(BinaryOp::Ushr),
        BinaryOperator::LessThan => Ok(BinaryOp::Lt),
        BinaryOperator::LessEqualThan => Ok(BinaryOp::Le),
        BinaryOperator::GreaterThan => Ok(BinaryOp::Gt),
        BinaryOperator::GreaterEqualThan => Ok(BinaryOp::Ge),
        BinaryOperator::Equality => Ok(BinaryOp::Eq),
        BinaryOperator::StrictEquality => Ok(BinaryOp::StrictEq),
        BinaryOperator::Inequality => Ok(BinaryOp::Neq),
        BinaryOperator::StrictInequality => Ok(BinaryOp::StrictNeq),
        BinaryOperator::BitwiseAnd => Ok(BinaryOp::BitAnd),
        BinaryOperator::BitwiseXOR => Ok(BinaryOp::BitXor),
        BinaryOperator::BitwiseOR => Ok(BinaryOp::BitOr),
        BinaryOperator::In => Ok(BinaryOp::In),
        BinaryOperator::Instanceof => Ok(BinaryOp::Instanceof),
    }
}

/// Lower unary operator
pub fn lower_unary_op(op: &UnaryOperator) -> Result<UnaryOp, LowerError> {
    match op {
        UnaryOperator::UnaryNegation => Ok(UnaryOp::Neg),
        UnaryOperator::UnaryPlus => Ok(UnaryOp::Plus),
        UnaryOperator::BitwiseNot => Ok(UnaryOp::BitNot),
        UnaryOperator::LogicalNot => Ok(UnaryOp::Not),
        UnaryOperator::Typeof => Ok(UnaryOp::Typeof),
        UnaryOperator::Void => Ok(UnaryOp::Void),
        UnaryOperator::Delete => Ok(UnaryOp::Delete),
    }
}

/// Lower logical operator (&&, ||, ??) to BinaryOp
pub fn lower_logical_op(op: &LogicalOperator) -> Result<BinaryOp, LowerError> {
    match op {
        LogicalOperator::And => Ok(BinaryOp::And),
        LogicalOperator::Or => Ok(BinaryOp::Or),
        LogicalOperator::Coalesce => Ok(BinaryOp::NullishCoalescing),
    }
}

/// Lower assignment operator to compound operator
#[allow(clippy::complexity)]
pub fn assign_op_to_bin(op: &AssignmentOperator) -> Result<CompoundOp, LowerError> {
    match op {
        AssignmentOperator::Assign => {
            Err(LowerError::new("Simple assignment has no compound form"))
        }
        AssignmentOperator::Addition => Ok(CompoundOp::Add),
        AssignmentOperator::Subtraction => Ok(CompoundOp::Sub),
        AssignmentOperator::Multiplication => Ok(CompoundOp::Mul),
        AssignmentOperator::Division => Ok(CompoundOp::Div),
        AssignmentOperator::Remainder => Ok(CompoundOp::Mod),
        AssignmentOperator::ShiftLeft => Ok(CompoundOp::Shl),
        AssignmentOperator::ShiftRight => Ok(CompoundOp::Shr),
        AssignmentOperator::ShiftRightZeroFill => Ok(CompoundOp::Ushr),
        AssignmentOperator::BitwiseAnd => Ok(CompoundOp::BitAnd),
        AssignmentOperator::BitwiseXOR => Ok(CompoundOp::BitXor),
        AssignmentOperator::BitwiseOR => Ok(CompoundOp::BitOr),
        AssignmentOperator::LogicalAnd => Ok(CompoundOp::LogicalAndAssign),
        AssignmentOperator::LogicalOr => Ok(CompoundOp::LogicalOrAssign),
        AssignmentOperator::LogicalNullish => Ok(CompoundOp::NullishCoalescingAssign),
        _ => Err(LowerError::new(format!(
            "Unsupported assign operator: {:?}",
            op
        ))),
    }
}
