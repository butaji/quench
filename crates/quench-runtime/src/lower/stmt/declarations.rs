//! Declaration lowering functions

use swc_ecma_ast as swc;
use crate::ast::{Class, ClassMember, PropertyKey, Statement, VarKind};

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
                decls.extend(lower_object_destructuring(kind, obj, init_expr, decls.len()));
            }
            _ => continue,
        }
    }
    wrap_decls(decls)
}

fn lower_fn_decl(func_decl: &swc::FnDecl) -> Option<Statement> {
    let name = func_decl.ident.sym.to_string();
    let params = func_decl.function.params.iter().map(|p| {
        match &p.pat {
            swc::Pat::Ident(ident) => ident.id.sym.to_string(),
            _ => "".to_string(),
        }
    }).collect();
    let body = func_decl.function.body.as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Some(Statement::FunctionDeclaration { name, params, body })
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
    let body: Vec<ClassMember> = class.body.iter().filter_map(lower_class_member_stmt).collect();
    Some(Class {
        name,
        super_class: super_class.map(Box::new),
        body,
    })
}

fn lower_class_member_stmt(member: &swc::ClassMember) -> Option<ClassMember> {
    use swc::ClassMember::*;
    match member {
        Constructor(params) => {
            let ps: Vec<String> = params.params.iter().filter_map(|p| {
                match p {
                    swc::ParamOrTsParamProp::Param(param) => {
                        match &param.pat {
                            swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                            _ => None,
                        }
                    }
                    swc::ParamOrTsParamProp::TsParamProp(_) => None,
                }
            }).collect();
            let body = params.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Some(ClassMember::Constructor { params: ps, body })
        }
        Method(method) => {
            let name = lower_prop_name_stmt(&method.key)?;
            let is_static = method.is_static;
            let ps: Vec<String> = method.function.params.iter().filter_map(|p| {
                match &p.pat {
                    swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                    _ => None,
                }
            }).collect();
            let body = method.function.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            match method.kind {
                swc::MethodKind::Getter => {
                    Some(ClassMember::Getter { name, body })
                }
                swc::MethodKind::Setter => {
                    let param = ps.first().cloned().unwrap_or_default();
                    Some(ClassMember::Setter { name, param, body })
                }
                swc::MethodKind::Method => {
                    if is_static {
                        Some(ClassMember::StaticMethod { name, params: ps, body })
                    } else {
                        Some(ClassMember::Method { name, params: ps, body })
                    }
                }
            }
        }
        PrivateMethod(_) => None,
        ClassProp(_) => None,
        PrivateProp(_) => None,
        Empty(_) => None,
        StaticBlock(_) => None,
        TsIndexSignature(_) => None,
        AutoAccessor(_) => None,
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
