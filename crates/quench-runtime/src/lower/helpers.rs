//! Lower helpers - shared utilities for SWC AST lowering

use crate::ast::{BinaryOp, CompoundOp, PropertyKey, UnaryOp};
use swc_atoms::Atom;
use swc_ecma_ast as swc;

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

/// Convert Atom to String using Display trait
pub fn atom_to_string(atom: &Atom) -> String {
    atom.to_string()
}

/// Convert Wtf8Atom to String using Display trait
pub fn wtf8_atom_to_string(atom: &swc_atoms::Wtf8Atom) -> String {
    atom.to_string_lossy().into_owned()
}

/// Lower binary operator
pub fn lower_bin_op(op: &swc::BinaryOp) -> Result<BinaryOp, LowerError> {
    lower_bin_op_arithmetic(op)
        .or_else(|| lower_bin_op_comparison(op))
        .or_else(|| lower_bin_op_bitwise(op))
        .ok_or_else(|| LowerError::new(format!("Unsupported binary operator: {:?}", op)))
}

#[allow(clippy::complexity)]
fn lower_bin_op_arithmetic(op: &swc::BinaryOp) -> Option<BinaryOp> {
    match op {
        swc::BinaryOp::Mul => Some(BinaryOp::Mul),
        swc::BinaryOp::Div => Some(BinaryOp::Div),
        swc::BinaryOp::Mod => Some(BinaryOp::Mod),
        swc::BinaryOp::Add => Some(BinaryOp::Add),
        swc::BinaryOp::Sub => Some(BinaryOp::Sub),
        swc::BinaryOp::LShift => Some(BinaryOp::Shl),
        swc::BinaryOp::RShift => Some(BinaryOp::Shr),
        swc::BinaryOp::ZeroFillRShift => Some(BinaryOp::Ushr),
        _ => None,
    }
}

#[allow(clippy::complexity)]
fn lower_bin_op_comparison(op: &swc::BinaryOp) -> Option<BinaryOp> {
    match op {
        swc::BinaryOp::Lt => Some(BinaryOp::Lt),
        swc::BinaryOp::LtEq => Some(BinaryOp::Le),
        swc::BinaryOp::Gt => Some(BinaryOp::Gt),
        swc::BinaryOp::GtEq => Some(BinaryOp::Ge),
        swc::BinaryOp::EqEq => Some(BinaryOp::Eq),
        swc::BinaryOp::EqEqEq => Some(BinaryOp::StrictEq),
        swc::BinaryOp::NotEq => Some(BinaryOp::Neq),
        swc::BinaryOp::NotEqEq => Some(BinaryOp::StrictNeq),
        _ => None,
    }
}

#[allow(clippy::complexity)]
fn lower_bin_op_bitwise(op: &swc::BinaryOp) -> Option<BinaryOp> {
    match op {
        swc::BinaryOp::BitAnd => Some(BinaryOp::BitAnd),
        swc::BinaryOp::BitXor => Some(BinaryOp::BitXor),
        swc::BinaryOp::BitOr => Some(BinaryOp::BitOr),
        swc::BinaryOp::LogicalAnd => Some(BinaryOp::And),
        swc::BinaryOp::LogicalOr => Some(BinaryOp::Or),
        swc::BinaryOp::NullishCoalescing => Some(BinaryOp::NullishCoalescing),
        swc::BinaryOp::In => Some(BinaryOp::In),
        swc::BinaryOp::InstanceOf => Some(BinaryOp::Instanceof),
        _ => None,
    }
}

/// Lower unary operator
pub fn lower_unary_op(op: &swc::UnaryOp) -> Result<UnaryOp, LowerError> {
    match op {
        swc::UnaryOp::Minus => Ok(UnaryOp::Neg),
        swc::UnaryOp::Plus => Ok(UnaryOp::Plus),
        swc::UnaryOp::Tilde => Ok(UnaryOp::BitNot),
        swc::UnaryOp::Bang => Ok(UnaryOp::Not),
        swc::UnaryOp::TypeOf => Ok(UnaryOp::Typeof),
        swc::UnaryOp::Void => Ok(UnaryOp::Void),
        swc::UnaryOp::Delete => Ok(UnaryOp::Delete),
    }
}

/// Lower assignment operator to compound operator
#[allow(clippy::complexity)]
pub fn assign_op_to_bin(op: &swc::AssignOp) -> Result<CompoundOp, LowerError> {
    match op {
        swc::AssignOp::AddAssign => Ok(CompoundOp::Add),
        swc::AssignOp::SubAssign => Ok(CompoundOp::Sub),
        swc::AssignOp::MulAssign => Ok(CompoundOp::Mul),
        swc::AssignOp::DivAssign => Ok(CompoundOp::Div),
        swc::AssignOp::ModAssign => Ok(CompoundOp::Mod),
        swc::AssignOp::LShiftAssign => Ok(CompoundOp::Shl),
        swc::AssignOp::RShiftAssign => Ok(CompoundOp::Shr),
        swc::AssignOp::ZeroFillRShiftAssign => Ok(CompoundOp::Ushr),
        swc::AssignOp::BitAndAssign => Ok(CompoundOp::BitAnd),
        swc::AssignOp::BitXorAssign => Ok(CompoundOp::BitXor),
        swc::AssignOp::BitOrAssign => Ok(CompoundOp::BitOr),
        swc::AssignOp::AndAssign => Ok(CompoundOp::LogicalAndAssign),
        swc::AssignOp::OrAssign => Ok(CompoundOp::LogicalOrAssign),
        swc::AssignOp::NullishAssign => Ok(CompoundOp::NullishCoalescingAssign),
        _ => Err(LowerError::new(format!(
            "Unsupported assign operator: {:?}",
            op
        ))),
    }
}

/// Lower property name
pub fn lower_prop_name(key: &swc::PropName) -> Result<PropertyKey, LowerError> {
    match key {
        swc::PropName::Str(s) => Ok(PropertyKey::String(wtf8_atom_to_string(&s.value))),
        swc::PropName::Ident(i) => Ok(PropertyKey::Ident(atom_to_string(&i.sym))),
        swc::PropName::Num(n) => Ok(PropertyKey::Number(n.value)),
        swc::PropName::Computed(_) => Err(LowerError::new("Computed property name not supported")),
        swc::PropName::BigInt(b) => Ok(PropertyKey::String(b.value.to_string())),
    }
}
