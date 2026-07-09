//! Statement lowering - convert SWC statements to runtime AST statements

use swc_ecma_ast as swc;
use crate::ast::{
    Class, ClassMember, Expression, PropertyKey, Statement, VarKind,
};
use super::control_flow::{
    lower_for_in_stmt, lower_for_of_stmt, lower_for_stmt, lower_if_stmt,
    lower_switch, lower_try_stmt, lower_while_stmt,
};
use super::expr::lower_expr;
use super::helpers::{atom_to_string, wtf8_atom_to_string, LowerError};
use super::pattern::{expand_nested_pattern, expand_nested_array_pattern};

/// Lower a swc Module to our runtime Program
pub fn lower_module(module: &swc::Module) -> Result<crate::ast::Program, LowerError> {
    let mut statements: Vec<Statement> = Vec::new();
    let mut export_stmts: Vec<Statement> = Vec::new();
    
    for item in &module.body {
        match lower_module_item(item) {
            Some(Statement::Export(stmt)) => export_stmts.push(*stmt),
            Some(stmt) => statements.push(stmt),
            None => {}
        }
    }
    
    // If we have export statements, add them at the end
    statements.extend(export_stmts);
    
    Ok(crate::ast::Program::Script(statements))
}

/// Lower a swc Script to our runtime Program
pub fn lower_script(script: &swc::Script) -> Result<crate::ast::Program, LowerError> {
    let statements: Vec<Statement> = script.body.iter()
        .filter_map(lower_stmt)
        .collect();
    Ok(crate::ast::Program::Script(statements))
}

/// Lower a swc ModuleItem to a Statement
fn lower_module_item(item: &swc::ModuleItem) -> Option<Statement> {
    match item {
        swc::ModuleItem::Stmt(stmt) => lower_stmt(stmt),
        swc::ModuleItem::ModuleDecl(decl) => lower_module_decl(decl),
    }
}

fn lower_module_decl(decl: &swc::ModuleDecl) -> Option<Statement> {
    match decl {
        // export default function foo() { ... }
        swc::ModuleDecl::ExportDefaultDecl(export_decl) => {
            match &export_decl.decl {
                swc::DefaultDecl::Fn(func_expr) => {
                    // For export default function, create a function declaration
                    let name = func_expr.ident.as_ref()
                        .map(|i| i.sym.to_string())
                        .unwrap_or_else(|| "default".to_string());
                    let params = func_expr.function.params.iter().map(|p| {
                        match &p.pat {
                            swc::Pat::Ident(ident) => ident.id.sym.to_string(),
                            _ => "".to_string(),
                        }
                    }).collect();
                    let body = func_expr.function.body.as_ref()
                        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                        .unwrap_or_default();
                    Some(Statement::FunctionDeclaration { name, params, body })
                }
                swc::DefaultDecl::Class(class_expr) => {
                    // For export default class
                    let name = class_expr.ident.as_ref()
                        .map(|i| i.sym.to_string())
                        .unwrap_or_else(|| "default".to_string());
                    let class = lower_class(&class_expr.class)?;
                    Some(Statement::ClassDeclaration { name, class })
                }
                swc::DefaultDecl::TsInterfaceDecl(_) => None,
            }
        }
        // export default expr
        swc::ModuleDecl::ExportDefaultExpr(expr) => {
            Some(Statement::Expression(Box::new(lower_expr(&expr.expr).ok()?)))
        }
        // export const/let/function/class declarations
        swc::ModuleDecl::ExportDecl(export_decl) => {
            lower_decl(&export_decl.decl)
        }
        // export { foo, bar }
        swc::ModuleDecl::ExportNamed(named) => {
            lower_export_named(named)
        }
        // export * from 'module' (not supported, skip)
        swc::ModuleDecl::ExportAll(_) => None,
        // import foo from 'module' (strip for CommonJS fallback)
        swc::ModuleDecl::Import(_) => None,
        // TypeScript export =
        swc::ModuleDecl::TsExportAssignment(_) => None,
        // TypeScript import =
        swc::ModuleDecl::TsImportEquals(_) => None,
        // TypeScript namespace export
        swc::ModuleDecl::TsNamespaceExport(_) => None,
    }
}

/// Lower export { foo, bar } to exports.foo = foo; exports.bar = bar;
fn lower_export_named(named: &swc::NamedExport) -> Option<Statement> {
    let mut stmts = Vec::new();
    for spec in &named.specifiers {
        match spec {
            swc::ExportSpecifier::Named(named_spec) => {
                let exported = named_spec.exported.as_ref()
                    .map(|e| match e {
                        swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
                        swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
                    })
                    .unwrap_or_else(|| {
                        match &named_spec.orig {
                            swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
                            swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
                        }
                    });
                let local = match &named_spec.orig {
                    swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
                    swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
                };
                stmts.push(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident(exported),
                        computed: false,
                    }),
                    right: Box::new(Expression::Identifier(local)),
                })));
            }
            swc::ExportSpecifier::Default(_) => {
                // export { default }
                stmts.push(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident("default".to_string()),
                        computed: false,
                    }),
                    right: Box::new(Expression::Identifier("default".to_string())),
                })));
            }
            swc::ExportSpecifier::Namespace(ns) => {
                // export * as ns from 'module'
                let name = match &ns.name {
                    swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
                    swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
                };
                stmts.push(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident(name.clone()),
                        computed: false,
                    }),
                    right: Box::new(Expression::Identifier(name)),
                })));
            }
        }
    }
    
    if stmts.is_empty() {
        None
    } else if stmts.len() == 1 {
        Some(Statement::Export(Box::new(stmts.into_iter().next().unwrap())))
    } else {
        Some(Statement::Export(Box::new(Statement::Block(stmts))))
    }
}

/// Lower a swc Stmt to our Statement
#[allow(unreachable_patterns)]
pub fn lower_stmt(stmt: &swc::Stmt) -> Option<Statement> {
    match stmt {
        swc::Stmt::Empty(_) => Some(Statement::Empty),
        swc::Stmt::Block(block) => {
            let stmts: Vec<Statement> = block.stmts.iter().filter_map(lower_stmt).collect();
            Some(Statement::Block(stmts))
        }
        swc::Stmt::Break(_) => Some(Statement::Break(None)),
        swc::Stmt::Continue(_) => Some(Statement::Continue(None)),
        swc::Stmt::Debugger(_) => Some(Statement::Empty),
        swc::Stmt::With(_) => None,
        swc::Stmt::Decl(decl) => lower_decl(decl),
        swc::Stmt::Return(ret) => {
            let expr = ret.arg.as_ref().and_then(|e| lower_expr(e).ok());
            Some(Statement::Return(expr.map(Box::new)))
        }
        swc::Stmt::Labeled(labeled) => lower_stmt(&labeled.body),
        swc::Stmt::If(if_stmt) => lower_if_stmt(if_stmt),
        swc::Stmt::Switch(switch) => lower_switch(switch),
        swc::Stmt::Throw(throw) => {
            let expr = lower_expr(&throw.arg).ok()?;
            Some(Statement::Throw(Box::new(expr)))
        }
        swc::Stmt::Try(try_stmt) => lower_try_stmt(try_stmt),
        swc::Stmt::While(while_stmt) => lower_while_stmt(while_stmt),
        swc::Stmt::DoWhile(_) => None,
        swc::Stmt::For(for_stmt) => lower_for_stmt(for_stmt),
        swc::Stmt::ForIn(for_in_stmt) => lower_for_in_stmt(for_in_stmt),
        swc::Stmt::ForOf(for_of_stmt) => lower_for_of_stmt(for_of_stmt),
        swc::Stmt::Expr(expr_stmt) => {
            let expr = lower_expr(&expr_stmt.expr).ok()?;
            Some(Statement::Expression(Box::new(expr)))
        }
        _ => None,
    }
}

/// Lower a declaration (function, var, const, let, class)
fn lower_decl(decl: &swc::Decl) -> Option<Statement> {
    match decl {
        swc::Decl::Fn(func_decl) => lower_fn_decl(func_decl),
        swc::Decl::Var(var_decl) => lower_var_decl(var_decl),
        swc::Decl::Class(class_decl) => lower_class_decl(class_decl),
        _ => None,
    }
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

fn lower_class(class: &swc::Class) -> Option<Class> {
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

fn lower_prop_name_stmt(key: &swc::PropName) -> Option<PropertyKey> {
    use super::helpers::wtf8_atom_to_string;
    match key {
        swc::PropName::Ident(i) => Some(PropertyKey::Ident(i.sym.to_string())),
        swc::PropName::Str(s) => Some(PropertyKey::String(wtf8_atom_to_string(&s.value))),
        swc::PropName::Num(n) => Some(PropertyKey::Number(n.value)),
        swc::PropName::Computed(_) => None,
        swc::PropName::BigInt(b) => Some(PropertyKey::String(format!("{}n", b.value))),
    }
}

fn lower_var_decl(var_decl: &swc::VarDecl) -> Option<Statement> {
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

fn lower_array_destructuring(
    kind: VarKind,
    arr: &swc::ArrayPat,
    init_expr: Option<Expression>,
    idx: usize,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    let temp_var_name = format!("__arr_src_{}", idx);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: temp_var_name.clone(),
        init: init_expr,
    });
    lower_array_elems(kind, arr, &temp_var_name, &mut stmts);
    stmts
}

fn lower_array_elems(
    kind: VarKind,
    arr: &swc::ArrayPat,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    for (i, elem) in arr.elems.iter().enumerate() {
        if let Some(elem) = elem {
            let member = array_member_access(temp_var_name, i);
            match elem {
                swc::Pat::Ident(id) => {
                    stmts.push(Statement::VarDeclaration {
                        kind,
                        name: atom_to_string(&id.id.sym),
                        init: Some(member),
                    });
                }
                _ => {
                    let elem_temp_name = format!("__arr_elem_{}", i);
                    stmts.push(Statement::VarDeclaration {
                        kind: VarKind::Const,
                        name: elem_temp_name.clone(),
                        init: Some(member),
                    });
                    stmts.extend(expand_nested_pattern(kind, elem, &elem_temp_name));
                }
            }
        }
    }
}

fn array_member_access(source_var: &str, index: usize) -> Expression {
    Expression::Member {
        object: Box::new(Expression::Identifier(source_var.to_string())),
        property: PropertyKey::Number(index as f64),
        computed: false,
    }
}

fn lower_object_destructuring(
    kind: VarKind,
    obj: &swc::ObjectPat,
    init_expr: Option<Expression>,
    idx: usize,
) -> Vec<Statement> {
    let mut stmts = Vec::new();
    let temp_var_name = format!("__obj_src_{}", idx);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: temp_var_name.clone(),
        init: init_expr,
    });
    lower_object_props(kind, obj, &temp_var_name, &mut stmts);
    stmts
}

fn lower_object_props(
    kind: VarKind,
    obj: &swc::ObjectPat,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    for prop in &obj.props {
        match prop {
            swc::ObjectPatProp::KeyValue(kv) => {
                handle_obj_kv_prop(kind, kv, temp_var_name, stmts);
            }
            swc::ObjectPatProp::Assign(assign) => {
                handle_obj_assign_prop(kind, assign, temp_var_name, stmts);
            }
            swc::ObjectPatProp::Rest(_) => {}
        }
    }
}

fn handle_obj_kv_prop(
    kind: VarKind,
    kv: &swc::KeyValuePatProp,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    let key_str = match &kv.key {
        swc::PropName::Ident(i) => atom_to_string(&i.sym),
        swc::PropName::Str(s) => wtf8_atom_to_string(&s.value),
        swc::PropName::Num(n) => n.value.to_string(),
        _ => return,
    };
    if key_str.is_empty() {
        return;
    }
    let kv_value_ref: &swc::Pat = &kv.value;
    let var_name = match kv_value_ref {
        swc::Pat::Ident(id) => atom_to_string(&id.id.sym),
        _ => format!("__obj_temp_{}", key_str),
    };
    let member = Expression::Member {
        object: Box::new(Expression::Identifier(temp_var_name.to_string())),
        property: PropertyKey::String(key_str.clone()),
        computed: false,
    };
    add_obj_destructure_stmts(kind, kv_value_ref, var_name, member, key_str, stmts);
}

fn handle_obj_assign_prop(
    kind: VarKind,
    assign: &swc::AssignPatProp,
    temp_var_name: &str,
    stmts: &mut Vec<Statement>,
) {
    let var_name = atom_to_string(&assign.key.sym);
    let member = Expression::Member {
        object: Box::new(Expression::Identifier(temp_var_name.to_string())),
        property: PropertyKey::Ident(var_name.clone()),
        computed: false,
    };
    stmts.push(Statement::VarDeclaration {
        kind,
        name: var_name,
        init: Some(member),
    });
}

fn add_obj_destructure_stmts(
    kind: VarKind,
    kv_value_ref: &swc::Pat,
    var_name: String,
    member: Expression,
    key_str: String,
    stmts: &mut Vec<Statement>,
) {
    match kv_value_ref {
        swc::Pat::Ident(_) => {
            stmts.push(Statement::VarDeclaration {
                kind,
                name: var_name,
                init: Some(member),
            });
        }
        swc::Pat::Object(nested_obj) => {
            handle_obj_nested(kind, member, key_str, nested_obj, stmts);
        }
        swc::Pat::Array(nested_arr) => {
            handle_arr_nested(kind, member, key_str, nested_arr, stmts);
        }
        _ => {
            stmts.push(Statement::VarDeclaration {
                kind,
                name: var_name,
                init: Some(member),
            });
        }
    }
}

fn handle_obj_nested(
    kind: VarKind,
    member: Expression,
    key_str: String,
    nested_obj: &swc::ObjectPat,
    stmts: &mut Vec<Statement>,
) {
    use super::pattern::expand_nested_object_pattern;
    let nested_temp_name = format!("__obj_prop_{}", key_str);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: nested_temp_name.clone(),
        init: Some(member),
    });
    stmts.extend(expand_nested_object_pattern(kind, nested_obj, &nested_temp_name));
}

fn handle_arr_nested(
    kind: VarKind,
    member: Expression,
    key_str: String,
    nested_arr: &swc::ArrayPat,
    stmts: &mut Vec<Statement>,
) {
    let nested_temp_name = format!("__obj_prop_{}", key_str);
    stmts.push(Statement::VarDeclaration {
        kind: VarKind::Const,
        name: nested_temp_name.clone(),
        init: Some(member),
    });
    stmts.extend(expand_nested_array_pattern(kind, nested_arr, &nested_temp_name));
}

/// Wrap declarations in appropriate statement(s).
/// For var declarations, return them as individual statements to avoid
/// creating a new block scope (var is function-scoped, not block-scoped).
/// For let/const, wrap in a Block since they're block-scoped.
fn wrap_decls(decls: Vec<Statement>) -> Option<Statement> {
    if decls.is_empty() {
        return Some(Statement::Empty);
    }
    if decls.len() == 1 {
        return Some(decls.into_iter().next().unwrap());
    }

    // Check if all declarations are var - if so, don't wrap in Block
    // to avoid creating a new scope (var is function-scoped)
    let all_var = decls.iter().all(|s| {
        matches!(s, Statement::VarDeclaration { kind: VarKind::Var, .. })
    });

    if all_var {
        // Return as Sequence of individual statements
        // This is handled specially in the stack machine
        Some(Statement::SequenceDecls(decls))
    } else {
        Some(Statement::Block(decls))
    }
}
