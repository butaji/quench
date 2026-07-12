//! Declaration lowering functions

use crate::ast::{Class, ClassMember, Expression, Param, PropertyKey, Statement, VarKind};
use swc_ecma_ast as swc;

use super::lower_stmt;
use crate::lower::expr::lower_expr;
use crate::lower::helpers::wtf8_atom_to_string;

/// Lower a declaration (function, var, const, let, class)
pub fn lower_decl(decl: &swc::Decl) -> Option<Statement> {
    match decl {
        swc::Decl::Fn(func_decl) => lower_fn_decl(func_decl),
        swc::Decl::Var(var_decl) => lower_var_decl(var_decl),
        swc::Decl::Class(class_decl) => lower_class_decl(class_decl),
        _ => None,
    }
}

#[allow(clippy::complexity)]
pub fn lower_var_decl(var_decl: &swc::VarDecl) -> Option<Statement> {
    use crate::lower::stmt::destructuring::{
        lower_array_destructuring, lower_object_destructuring, wrap_decls,
    };

    let kind = match var_decl.kind {
        swc::VarDeclKind::Var => VarKind::Var,
        swc::VarDeclKind::Let => VarKind::Let,
        swc::VarDeclKind::Const => VarKind::Const,
    };
    let mut decls = Vec::new();
    for binding in &var_decl.decls {
        let init_expr = binding.init.as_ref().and_then(|e| lower_expr(e).ok());
        match &binding.name {
            swc::Pat::Ident(ident) => {
                decls.push(Statement::VarDeclaration {
                    kind,
                    name: ident.id.sym.to_string(),
                    init: init_expr,
                });
            }
            swc::Pat::Array(arr) => {
                decls.extend(lower_array_destructuring(kind, arr, init_expr, decls.len()));
            }
            swc::Pat::Object(obj) => {
                decls.extend(lower_object_destructuring(
                    kind,
                    obj,
                    init_expr,
                    decls.len(),
                ));
            }
            _ => continue,
        }
    }
    wrap_decls(decls)
}

fn lower_fn_decl(func_decl: &swc::FnDecl) -> Option<Statement> {
    let name = func_decl.ident.sym.to_string();
    let params: Vec<Param> = func_decl
        .function
        .params
        .iter()
        .map(|p| lower_param_decl(&p.pat))
        .collect();
    let body = func_decl
        .function
        .body
        .as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Some(Statement::FunctionDeclaration { name, params, body })
}

pub fn lower_param_decl(pat: &swc::Pat) -> Param {
    match pat {
        swc::Pat::Ident(ident) => Param::new(ident.id.sym.as_ref()),
        swc::Pat::Assign(assign) => {
            let name = match assign.left.as_ref() {
                swc::Pat::Ident(ident) => ident.id.sym.to_string(),
                _ => "arg".to_string(),
            };
            let default = lower_expr(&assign.right).ok().map(Box::new);
            Param { name, default }
        }
        _ => Param::new("arg"),
    }
}

fn lower_class_decl(class_decl: &swc::ClassDecl) -> Option<Statement> {
    let name = class_decl.ident.sym.to_string();
    let class = lower_class(&class_decl.class)?;
    Some(Statement::ClassDeclaration { name, class })
}

pub fn lower_class(class: &swc::Class) -> Option<Class> {
    // Class name is not stored in the Class struct, only in ClassDecl
    let name: Option<String> = None;
    let super_class = class.super_class.as_ref().and_then(|e| lower_expr(e).ok());
    let body: Vec<ClassMember> = class
        .body
        .iter()
        .filter_map(lower_class_member_stmt)
        .collect();
    Some(Class {
        name,
        super_class: super_class.map(Box::new),
        body,
    })
}

fn lower_class_member_stmt(member: &swc::ClassMember) -> Option<ClassMember> {
    use swc::ClassMember::*;
    match member {
        Constructor(params) => lower_constructor_stmt(params),
        Method(method) => lower_method_stmt(method),
        ClassProp(prop) => lower_class_prop_stmt(prop),
        PrivateMethod(_) | PrivateProp(_) | Empty(_) | StaticBlock(_) | TsIndexSignature(_)
        | AutoAccessor(_) => None,
    }
}

fn lower_constructor_stmt(params: &swc::Constructor) -> Option<ClassMember> {
    let ps: Vec<String> = params
        .params
        .iter()
        .filter_map(|p| match p {
            swc::ParamOrTsParamProp::Param(param) => match &param.pat {
                swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                _ => None,
            },
            swc::ParamOrTsParamProp::TsParamProp(_) => None,
        })
        .collect();
    let body = params
        .body
        .as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Some(ClassMember::Constructor { params: ps, body })
}

#[allow(clippy::complexity)]
fn lower_method_stmt(method: &swc::ClassMethod) -> Option<ClassMember> {
    let name = lower_prop_name_stmt(&method.key)?;
    let is_static = method.is_static;
    let ps: Vec<String> = method
        .function
        .params
        .iter()
        .filter_map(|p| match &p.pat {
            swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
            _ => None,
        })
        .collect();
    let body = method
        .function
        .body
        .as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    match method.kind {
        swc::MethodKind::Getter => Some(ClassMember::Getter { name, body }),
        swc::MethodKind::Setter => {
            let param = ps.first().cloned().unwrap_or_default();
            Some(ClassMember::Setter { name, param, body })
        }
        swc::MethodKind::Method => {
            if is_static {
                Some(ClassMember::StaticMethod {
                    name,
                    params: ps,
                    body,
                })
            } else {
                Some(ClassMember::Method {
                    name,
                    params: ps,
                    body,
                })
            }
        }
    }
}

fn lower_class_prop_stmt(prop: &swc::ClassProp) -> Option<ClassMember> {
    let name = lower_prop_name_stmt(&prop.key)?;
    let value = match &prop.value {
        Some(expr) => lower_expr(expr).ok()?,
        None => Expression::Undefined,
    };
    if prop.is_static {
        Some(ClassMember::StaticField { name, value })
    } else {
        Some(ClassMember::Field { name, value })
    }
}

pub fn lower_prop_name_stmt(key: &swc::PropName) -> Option<PropertyKey> {
    match key {
        swc::PropName::Ident(i) => Some(PropertyKey::Ident(i.sym.to_string())),
        swc::PropName::Str(s) => Some(PropertyKey::String(wtf8_atom_to_string(&s.value))),
        swc::PropName::Num(n) => Some(PropertyKey::Number(n.value)),
        swc::PropName::Computed(_) => None,
        swc::PropName::BigInt(b) => Some(PropertyKey::String(format!("{}n", b.value))),
    }
}
