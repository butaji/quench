//! Lower helpers - shared utilities for SWC AST lowering

use swc_atoms::Atom;
use swc_ecma_ast as swc;
use crate::ast::{BinaryOp, CompoundOp, PropertyKey, UnaryOp};

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
        LowerError { message: message.into() }
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
    match op {
        swc::BinaryOp::Mul => Ok(BinaryOp::Mul),
        swc::BinaryOp::Div => Ok(BinaryOp::Div),
        swc::BinaryOp::Mod => Ok(BinaryOp::Mod),
        swc::BinaryOp::Add => Ok(BinaryOp::Add),
        swc::BinaryOp::Sub => Ok(BinaryOp::Sub),
        swc::BinaryOp::LShift => Ok(BinaryOp::Shl),
        swc::BinaryOp::RShift => Ok(BinaryOp::Shr),
        swc::BinaryOp::ZeroFillRShift => Ok(BinaryOp::Ushr),
        swc::BinaryOp::Lt => Ok(BinaryOp::Lt),
        swc::BinaryOp::LtEq => Ok(BinaryOp::Le),
        swc::BinaryOp::Gt => Ok(BinaryOp::Gt),
        swc::BinaryOp::GtEq => Ok(BinaryOp::Ge),
        swc::BinaryOp::EqEq => Ok(BinaryOp::Eq),
        swc::BinaryOp::EqEqEq => Ok(BinaryOp::StrictEq),
        swc::BinaryOp::NotEq => Ok(BinaryOp::Neq),
        swc::BinaryOp::NotEqEq => Ok(BinaryOp::StrictNeq),
        swc::BinaryOp::BitAnd => Ok(BinaryOp::BitAnd),
        swc::BinaryOp::BitXor => Ok(BinaryOp::BitXor),
        swc::BinaryOp::BitOr => Ok(BinaryOp::BitOr),
        swc::BinaryOp::LogicalAnd => Ok(BinaryOp::And),
        swc::BinaryOp::LogicalOr => Ok(BinaryOp::Or),
        swc::BinaryOp::NullishCoalescing => Ok(BinaryOp::NullishCoalescing),
        swc::BinaryOp::In => Ok(BinaryOp::In),
        swc::BinaryOp::InstanceOf => Ok(BinaryOp::Instanceof),
        _ => Err(LowerError::new(format!("Unsupported binary operator: {:?}", op))),
    }
}

/// Lower unary operator
pub fn lower_unary_op(op: &swc::UnaryOp) -> Result<UnaryOp, LowerError> {
    match op {
        swc::UnaryOp::Minus => Ok(UnaryOp::Neg),
        swc::UnaryOp::Plus => Err(LowerError::new("Unary + not supported")),
        swc::UnaryOp::Tilde => Ok(UnaryOp::BitNot),
        swc::UnaryOp::Bang => Ok(UnaryOp::Not),
        swc::UnaryOp::TypeOf => Ok(UnaryOp::Typeof),
        swc::UnaryOp::Void => Ok(UnaryOp::Void),
        swc::UnaryOp::Delete => Err(LowerError::new("Delete not supported")),
    }
}

/// Lower assignment operator to compound operator
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
        _ => Err(LowerError::new(format!("Unsupported assign operator: {:?}", op))),
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
