//! Statement lowering - convert SWC statements to runtime AST statements

use swc_ecma_ast as swc;
use crate::ast::{
    BinaryOp, Expression, ForInit, PropertyKey, Statement, VarKind,
};
use super::expr::lower_expr;
use super::helpers::{atom_to_string, wtf8_atom_to_string, LowerError};
use super::pattern::expand_nested_pattern;

/// Lower a swc Module to our runtime Program
pub fn lower_module(module: &swc::Module) -> Result<crate::ast::Program, LowerError> {
    let statements: Vec<Statement> = module.body.iter()
        .filter_map(lower_module_item)
        .collect();
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
        swc::ModuleDecl::ExportDefaultDecl(_) => None,
        swc::ModuleDecl::ExportDefaultExpr(expr) => {
            Some(Statement::Expression(Box::new(lower_expr(&expr.expr).ok()?)))
        }
        _ => None,
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

fn lower_if_stmt(if_stmt: &swc::IfStmt) -> Option<Statement> {
    let condition = lower_expr(&if_stmt.test).ok()?;
    let consequent = Box::new(lower_stmt(&if_stmt.cons).unwrap_or(Statement::Empty));
    let alternate = if_stmt.alt.as_ref().map(|a| {
        Box::new(lower_stmt(a).unwrap_or(Statement::Empty))
    });
    Some(Statement::If {
        condition: Box::new(condition),
        consequent,
        alternate,
    })
}

fn lower_while_stmt(while_stmt: &swc::WhileStmt) -> Option<Statement> {
    let condition = lower_expr(&while_stmt.test).ok()?;
    let body = Box::new(lower_stmt(&while_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::While { condition: Box::new(condition), body })
}

fn lower_for_stmt(for_stmt: &swc::ForStmt) -> Option<Statement> {
    let init = for_stmt.init.as_ref().and_then(lower_for_init);
    let condition = for_stmt.test.as_ref().and_then(|e| lower_expr(e).ok()).map(Box::new);
    let update = for_stmt.update.as_ref().and_then(|e| lower_expr(e).ok()).map(Box::new);
    let body = Box::new(lower_stmt(&for_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::For { init, condition, update, body })
}

fn lower_for_in_stmt(for_in_stmt: &swc::ForInStmt) -> Option<Statement> {
    let left = lower_for_lhs(&for_in_stmt.left)?;
    let iterable = lower_expr(&for_in_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_in_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::Expression(Box::new(Expression::ForIn {
        variable: Box::new(left),
        object: Box::new(iterable),
        body,
    })))
}

fn lower_for_of_stmt(for_of_stmt: &swc::ForOfStmt) -> Option<Statement> {
    let left = lower_for_lhs(&for_of_stmt.left)?;
    let iterable = lower_expr(&for_of_stmt.right).ok()?;
    let body = Box::new(lower_stmt(&for_of_stmt.body).unwrap_or(Statement::Empty));
    Some(Statement::Expression(Box::new(Expression::ForOf {
        variable: Box::new(left),
        iterable: Box::new(iterable),
        body,
    })))
}

fn lower_try_stmt(try_stmt: &swc::TryStmt) -> Option<Statement> {
    let body = Box::new(
        lower_stmt(&swc::Stmt::Block(try_stmt.block.clone()))
            .unwrap_or(Statement::Empty)
    );
    let catch_param = try_stmt.handler.as_ref().and_then(|catch| {
        catch.param.as_ref().and_then(|pat| {
            match pat {
                swc::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
                _ => None,
            }
        })
    });
    let handler = if let Some(catch) = &try_stmt.handler {
        Box::new(
            lower_stmt(&swc::Stmt::Block(catch.body.clone()))
                .unwrap_or(Statement::Empty)
        )
    } else {
        Box::new(Statement::Empty)
    };
    Some(Statement::TryCatch { body, param: catch_param, handler })
}

/// Lower a declaration (function, var, const, let, class)
fn lower_decl(decl: &swc::Decl) -> Option<Statement> {
    match decl {
        swc::Decl::Fn(func_decl) => lower_fn_decl(func_decl),
        swc::Decl::Var(var_decl) => lower_var_decl(var_decl),
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
    for (i, elem) in arr.elems.iter().enumerate() {
        match elem {
            Some(elem) => {
                let member = Expression::Member {
                    object: Box::new(Expression::Identifier(temp_var_name.clone())),
                    property: PropertyKey::Number(i as f64),
                    computed: false,
                };
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
            None => {}
        }
    }
    stmts
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
    for prop in &obj.props {
        match prop {
            swc::ObjectPatProp::KeyValue(kv) => {
                let key_str = match &kv.key {
                    swc::PropName::Ident(i) => atom_to_string(&i.sym),
                    swc::PropName::Str(s) => wtf8_atom_to_string(&s.value),
                    swc::PropName::Num(n) => n.value.to_string(),
                    _ => continue,
                };
                if key_str.is_empty() {
                    continue;
                }
                let kv_value_ref: &swc::Pat = &kv.value;
                let var_name = match kv_value_ref {
                    swc::Pat::Ident(id) => atom_to_string(&id.id.sym),
                    _ => format!("__obj_temp_{}", key_str),
                };
                let member = Expression::Member {
                    object: Box::new(Expression::Identifier(temp_var_name.clone())),
                    property: PropertyKey::String(key_str.clone()),
                    computed: false,
                };
                add_obj_destructure_stmts(kind, kv_value_ref, var_name, member, key_str, &mut stmts);
            }
            swc::ObjectPatProp::Assign(assign) => {
                let var_name = atom_to_string(&assign.key.sym);
                let member = Expression::Member {
                    object: Box::new(Expression::Identifier(temp_var_name.clone())),
                    property: PropertyKey::Ident(var_name.clone()),
                    computed: false,
                };
                stmts.push(Statement::VarDeclaration {
                    kind,
                    name: var_name,
                    init: Some(member),
                });
            }
            swc::ObjectPatProp::Rest(_) => {}
        }
    }
    stmts
}

fn add_obj_destructure_stmts(
    kind: VarKind,
    kv_value_ref: &swc::Pat,
    var_name: String,
    member: Expression,
    key_str: String,
    stmts: &mut Vec<Statement>,
) {
    use super::pattern::expand_nested_object_pattern;
    match kv_value_ref {
        swc::Pat::Ident(_) => {
            stmts.push(Statement::VarDeclaration {
                kind,
                name: var_name,
                init: Some(member),
            });
        }
        swc::Pat::Object(nested_obj) => {
            let nested_temp_name = format!("__obj_prop_{}", key_str);
            stmts.push(Statement::VarDeclaration {
                kind: VarKind::Const,
                name: nested_temp_name.clone(),
                init: Some(member),
            });
            stmts.extend(expand_nested_object_pattern(kind, nested_obj, &nested_temp_name));
        }
        swc::Pat::Array(nested_arr) => {
            let nested_temp_name = format!("__obj_prop_{}", key_str);
            stmts.push(Statement::VarDeclaration {
                kind: VarKind::Const,
                name: nested_temp_name.clone(),
                init: Some(member),
            });
            stmts.extend(expand_nested_array_pattern(kind, nested_arr, &nested_temp_name));
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

fn expand_nested_array_pattern(
    kind: VarKind,
    arr: &swc::ArrayPat,
    source_var: &str,
) -> Vec<Statement> {
    use super::pattern::expand_nested_array_pattern;
    expand_nested_array_pattern(kind, arr, source_var)
}

fn wrap_decls(decls: Vec<Statement>) -> Option<Statement> {
    if decls.is_empty() {
        Some(Statement::Empty)
    } else if decls.len() == 1 {
        Some(decls.into_iter().next().unwrap())
    } else {
        Some(Statement::Block(decls))
    }
}

fn lower_switch(switch: &swc::SwitchStmt) -> Option<Statement> {
    let discriminant = lower_expr(&switch.discriminant).ok()?;
    let mut current: Option<Statement> = None;
    for case in switch.cases.iter().rev() {
        let case_body = case.cons.iter()
            .filter_map(lower_stmt)
            .collect::<Vec<_>>();
        let new_stmt = if let Some(test) = &case.test {
            let test_expr = lower_expr(test).ok()?;
            Statement::If {
                condition: Box::new(Expression::Binary {
                    op: BinaryOp::StrictEq,
                    left: Box::new(discriminant.clone()),
                    right: Box::new(test_expr),
                }),
                consequent: Box::new(Statement::Block(case_body)),
                alternate: current.map(Box::new),
            }
        } else {
            Statement::Block(case_body)
        };
        current = Some(new_stmt);
    }
    current.or(Some(Statement::Empty))
}

fn lower_for_init(init: &swc::VarDeclOrExpr) -> Option<ForInit> {
    match init {
        swc::VarDeclOrExpr::VarDecl(decl) => {
            let first = decl.decls.first()?;
            let kind = match decl.kind {
                swc::VarDeclKind::Var => VarKind::Var,
                swc::VarDeclKind::Let => VarKind::Let,
                swc::VarDeclKind::Const => VarKind::Const,
            };
            let name = match &first.name {
                swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
                _ => return None,
            };
            let init = first.init.as_ref().and_then(|e| lower_expr(e).ok());
            Some(ForInit::VarDeclaration { kind, name, init })
        }
        swc::VarDeclOrExpr::Expr(expr) => {
            Some(ForInit::Expression(Box::new(lower_expr(expr).ok()?)))
        }
    }
}

/// Lower the left-hand side of a for-in/for-of loop
fn lower_for_lhs(left: &swc::ForHead) -> Option<Expression> {
    match left {
        swc::ForHead::VarDecl(decl) => {
            let first = decl.decls.first()?;
            match &first.name {
                swc::Pat::Ident(ident) => {
                    Some(Expression::Identifier(atom_to_string(&ident.id.sym)))
                }
                _ => None,
            }
        }
        swc::ForHead::Pat(pat) => {
            match pat.as_ref() {
                swc::Pat::Ident(ident) => {
                    Some(Expression::Identifier(atom_to_string(&ident.id.sym)))
                }
                _ => None,
            }
        }
        swc::ForHead::UsingDecl(_) => None,
    }
}
