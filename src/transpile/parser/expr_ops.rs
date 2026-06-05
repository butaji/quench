//! Operator conversion utilities

use crate::transpile::hir;
use oxc_ast::ast::*;

pub fn unary_op(op: &UnaryOperator) -> hir::UnaryOp {
    match op {
        UnaryOperator::Minus => hir::UnaryOp::Minus,
        UnaryOperator::Plus => hir::UnaryOp::Plus,
        UnaryOperator::LogicalNot => hir::UnaryOp::Not,
        UnaryOperator::BitwiseNot => hir::UnaryOp::BitNot,
        UnaryOperator::Typeof => hir::UnaryOp::Typeof,
        UnaryOperator::Void => hir::UnaryOp::Void,
        UnaryOperator::Delete => hir::UnaryOp::Delete,
    }
}

pub fn binary_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    arith_op(op).or_else(|| shift_op(op)).or_else(|| cmp_op(op)).or_else(|| eq_op(op)).or_else(|| bit_op(op)).or_else(|| rel_op(op))
}

fn arith_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::Addition => Some(Add),
        BinaryOperator::Subtraction => Some(Sub),
        BinaryOperator::Multiplication => Some(Mul),
        BinaryOperator::Division => Some(Div),
        BinaryOperator::Remainder => Some(Mod),
        BinaryOperator::Exponentiation => Some(Exp),
        _ => None,
    }
}

fn shift_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::LeftShift => Some(Shl),
        BinaryOperator::RightShift => Some(Shr),
        BinaryOperator::UnsignedRightShift => Some(UShr),
        _ => None,
    }
}

fn cmp_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::LessThan => Some(Lt),
        BinaryOperator::GreaterThan => Some(Gt),
        BinaryOperator::LessThanOrEqual => Some(Lte),
        BinaryOperator::GreaterThanOrEqual => Some(Gte),
        _ => None,
    }
}

fn eq_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::Equality => Some(Eq),
        BinaryOperator::StrictEquality => Some(StrictEq),
        BinaryOperator::Inequality => Some(Neq),
        BinaryOperator::StrictInequality => Some(StrictNeq),
        _ => None,
    }
}

fn bit_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::BitwiseAnd => Some(BitAnd),
        BinaryOperator::BitwiseXor => Some(BitXor),
        BinaryOperator::BitwiseOr => Some(BitOr),
        _ => None,
    }
}

fn rel_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::In => Some(In),
        BinaryOperator::Instanceof => Some(Instanceof),
        _ => None,
    }
}

pub fn assign_op(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    arith_assign(op).or_else(|| shift_assign(op)).or_else(|| bit_assign(op))
}

fn arith_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::Equal => Some(Assign),
        AssignmentOperator::Plus => Some(AddAssign),
        AssignmentOperator::Minus => Some(SubAssign),
        AssignmentOperator::Mul => Some(MulAssign),
        AssignmentOperator::Div => Some(DivAssign),
        AssignmentOperator::Remainder => Some(ModAssign),
        AssignmentOperator::Exponent => Some(ExpAssign),
        _ => None,
    }
}

fn shift_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::LeftShift => Some(ShlAssign),
        AssignmentOperator::RightShift => Some(ShrAssign),
        AssignmentOperator::UnsignedRightShift => Some(UShrAssign),
        _ => None,
    }
}

fn bit_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::BitwiseAnd => Some(BitAndAssign),
        AssignmentOperator::BitwiseXor => Some(BitXorAssign),
        AssignmentOperator::BitwiseOr => Some(BitOrAssign),
        _ => None,
    }
}
