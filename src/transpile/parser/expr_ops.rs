//! Operator conversion utilities

use crate::transpile::hir;
use oxc_syntax::operator::*;

pub fn unary_op(op: &UnaryOperator) -> hir::UnaryOp {
    match op {
        UnaryOperator::UnaryNegation => hir::UnaryOp::Minus,
        UnaryOperator::UnaryPlus => hir::UnaryOp::Plus,
        UnaryOperator::LogicalNot => hir::UnaryOp::Not,
        UnaryOperator::BitwiseNot => hir::UnaryOp::BitNot,
        UnaryOperator::Typeof => hir::UnaryOp::Typeof,
        UnaryOperator::Void => hir::UnaryOp::Void,
        UnaryOperator::Delete => hir::UnaryOp::Delete,
    }
}

pub fn binary_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    
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
        BinaryOperator::Exponential => Some(Exp),
        _ => None,
    }
}

fn shift_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::ShiftLeft => Some(Shl),
        BinaryOperator::ShiftRight => Some(Shr),
        BinaryOperator::ShiftRightZeroFill => Some(UShr),
        _ => None,
    }
}

fn cmp_op(op: &BinaryOperator) -> Option<hir::BinaryOp> {
    use hir::BinaryOp::*;
    match op {
        BinaryOperator::LessThan => Some(Lt),
        BinaryOperator::GreaterThan => Some(Gt),
        BinaryOperator::LessEqualThan => Some(Lte),
        BinaryOperator::GreaterEqualThan => Some(Gte),
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
        BinaryOperator::BitwiseXOR => Some(BitXor),
        BinaryOperator::BitwiseOR => Some(BitOr),
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
    
    arith_assign(op)
        .or_else(|| shift_assign(op))
        .or_else(|| bit_assign(op))
        .or_else(|| logical_assign(op))
}

fn arith_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::Assign => Some(Assign),
        AssignmentOperator::Addition => Some(AddAssign),
        AssignmentOperator::Subtraction => Some(SubAssign),
        AssignmentOperator::Multiplication => Some(MulAssign),
        AssignmentOperator::Division => Some(DivAssign),
        AssignmentOperator::Remainder => Some(ModAssign),
        AssignmentOperator::Exponential => Some(ExpAssign),
        _ => None,
    }
}

fn shift_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::ShiftLeft => Some(ShlAssign),
        AssignmentOperator::ShiftRight => Some(ShrAssign),
        AssignmentOperator::ShiftRightZeroFill => Some(UShrAssign),
        _ => None,
    }
}

fn bit_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::BitwiseAnd => Some(BitAndAssign),
        AssignmentOperator::BitwiseXOR => Some(BitXorAssign),
        AssignmentOperator::BitwiseOR => Some(BitOrAssign),
        _ => None,
    }
}

fn logical_assign(op: &AssignmentOperator) -> Option<hir::AssignOp> {
    use hir::AssignOp::*;
    match op {
        AssignmentOperator::LogicalOr => Some(LogicalOrAssign),
        AssignmentOperator::LogicalAnd => Some(LogicalAndAssign),
        AssignmentOperator::LogicalNullish => Some(NullishCoalescingAssign),
        _ => None,
    }
}
