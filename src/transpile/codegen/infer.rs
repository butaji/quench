//! Type inference utilities

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct TypeInfer;

impl TypeInfer {
    pub fn infer_expr_type(cg: &CodeGenerator, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Null => Some(Type::Null),
            Expr::Boolean(_) => Some(Type::Boolean),
            Expr::Number(_) => Some(Type::Number),
            Expr::String(_) => Some(Type::String),
            Expr::BigInt(_) => Some(Type::String),
            Expr::Array { .. } => Some(Type::Array { elem: Box::new(Type::Unknown) }),
            Expr::Object { .. } => Some(Type::Object { members: vec![] }),
            Expr::Ident { name } => cg.lookup_type_def(name),
            Expr::Member { object, .. } => Self::infer_member_type(cg, object),
            Expr::Call { callee, .. } => Self::infer_call_type(cg, callee),
            Expr::Bin { op, .. } => Some(Self::binary_result_type(op)),
            Expr::Unary { op, .. } => Some(Self::unary_result_type(op)),
            Expr::Cond { consequent, .. } => Self::infer_expr_type(cg, consequent),
            Expr::Function { decl } => {
                decl.return_type.clone()
            }
            Expr::Arrow { .. } => Some(Type::Function { params: vec![], ret: Box::new(Type::Unknown), generics: vec![] }),
            Expr::JSX(_) => Some(Type::String),
            _ => None,
        }
    }

    fn infer_member_type(cg: &CodeGenerator, object: &Expr) -> Option<Type> {
        let obj_type = Self::infer_expr_type(cg, object)?;
        match obj_type {
            Type::Object { members: _ } => Some(Type::Unknown),
            Type::Ref { name, generics: _ } => {
                let type_def = cg.type_defs.get(&name)?;
                if let Type::Object { members: _ } = &type_def.type_ {
                    Some(Type::Unknown)
                } else { None }
            }
            _ => None,
        }
    }

    fn infer_call_type(_cg: &CodeGenerator, _callee: &Expr) -> Option<Type> {
        Some(Type::Unknown)
    }

    fn binary_result_type(op: &BinaryOp) -> Type {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Exp => Type::Number,
            _ => Type::Boolean,
        }
    }

    fn unary_result_type(op: &UnaryOp) -> Type {
        match op {
            UnaryOp::Minus | UnaryOp::Plus => Type::Number,
            UnaryOp::Not | UnaryOp::BitNot => Type::Boolean,
            UnaryOp::TypeOf => Type::String,
            UnaryOp::Void => Type::Undefined,
        }
    }
}
