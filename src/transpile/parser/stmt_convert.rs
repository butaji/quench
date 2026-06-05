//! Statement conversion - stmt_to_hir_stmt and helpers

use crate::transpile::hir;
use crate::transpile::parser::expr::{arr_elems, convert_binding_pattern, convert_expr, conv_object, conv_template};

use oxc_ast::ast::*;

pub fn stmt_to_hir_stmt(s: &Statement) -> hir::Stmt {
    if let Statement::ExpressionStatement(e) = s { return convert_expr_stmt(e); }
    if let Statement::IfStatement(stmt) = s { return convert_if_stmt(stmt); }
    if let Statement::LoopStatement(stmt) = s { return convert_loop_stmt(stmt); }
    stmt_to_hir_stmt_impl(s)
}

fn stmt_to_hir_stmt_impl(s: &Statement) -> hir::Stmt {
    if let Statement::SwitchStatement(stmt) = s { return convert_switch_stmt(stmt); }
    if let Statement::TryStatement(stmt) = s { return convert_try_stmt(stmt); }
    stmt_to_hir_stmt_impl2(s)
}

fn stmt_to_hir_stmt_impl2(s: &Statement) -> hir::Stmt {
    if let Statement::LabeledStatement(stmt) = s { return convert_labeled_stmt(stmt); }
    if let Statement::ReturnStatement(r) = s { return convert_return_stmt(r); }
    if let Statement::JumpStatement(stmt) = s { return convert_jump_stmt(stmt); }
    if let Statement::BlockStatement(b) = s { return hir::Stmt::Block { stmts: b.body.iter().map(stmt_to_hir_stmt).collect() }; }
    stmt_to_hir_stmt_impl3(s)
}

fn stmt_to_hir_stmt_impl3(s: &Statement) -> hir::Stmt {
    if let Statement::VariableDeclaration(v) = s { return convert_var_stmt(v); }
    if let Statement::ClassDeclaration(c) = s { return convert_class_decl_stmt(c); }
    if let Statement::FunctionDeclaration(f) = s { return convert_func_decl_stmt(f); }
    if let Statement::ThrowStatement(stmt) = s { return convert_throw_stmt(stmt); }
    hir::Stmt::Empty
}

fn convert_loop_stmt(stmt: &LoopStatement) -> hir::Stmt {
    match stmt {
        LoopStatement::ForStatement(s) => convert_for_stmt(s),
        LoopStatement::ForInStatement(s) => convert_for_in_stmt(s),
        LoopStatement::ForOfStatement(s) => convert_for_of_stmt(s),
        LoopStatement::WhileStatement(s) => convert_while_stmt(s),
        LoopStatement::DoWhileStatement(s) => convert_do_while_stmt(s),
    }
}

fn convert_jump_stmt(stmt: &JumpStatement) -> hir::Stmt {
    match stmt {
        JumpStatement::BreakStatement(_) => hir::Stmt::Break { label: None },
        JumpStatement::ContinueStatement(_) => hir::Stmt::Continue { label: None },
    }
}

fn convert_expr_stmt(e: &ExpressionStatement) -> hir::Stmt {
    hir::Stmt::Expr { expr: convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined) }
}

fn convert_if_stmt(stmt: &IfStatement) -> hir::Stmt {
    hir::Stmt::If {
        test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
        consequent: Box::new(stmt_to_hir_stmt(&stmt.consequent)),
        alternate: stmt.alternate.as_ref().map(|a| Box::new(stmt_to_hir_stmt(a))),
    }
}

fn convert_while_stmt(stmt: &WhileStatement) -> hir::Stmt {
    hir::Stmt::While {
        test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
        body: Box::new(stmt_to_hir_stmt(&stmt.body)),
    }
}

fn convert_do_while_stmt(stmt: &DoWhileStatement) -> hir::Stmt {
    hir::Stmt::DoWhile {
        body: Box::new(stmt_to_hir_stmt(&stmt.body)),
        test: convert_expr(&stmt.test).unwrap_or(hir::Expr::Undefined),
    }
}

fn convert_throw_stmt(stmt: &ThrowStatement) -> hir::Stmt {
    hir::Stmt::Throw { arg: convert_expr(&stmt.argument).unwrap_or(hir::Expr::Undefined) }
}

fn convert_labeled_stmt(stmt: &LabeledStatement) -> hir::Stmt {
    hir::Stmt::Labeled {
        label: stmt.label.name.to_string(),
        body: Box::new(stmt_to_hir_stmt(&stmt.body)),
    }
}

fn convert_return_stmt(r: &ReturnStatement) -> hir::Stmt {
    hir::Stmt::Return { arg: r.argument.as_ref().and_then(|a| convert_expr(a).ok()) }
}

fn var_kind(kind: VariableDeclarationKind) -> hir::VariableKind {
    match kind {
        VariableDeclarationKind::Const => hir::VariableKind::Const,
        VariableDeclarationKind::Let => hir::VariableKind::Let,
        VariableDeclarationKind::Var => hir::VariableKind::Var,
        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => hir::VariableKind::Var,
    }
}

fn convert_for_init_expr(init: &Option<ForStatementInit>) -> Option<hir::ForInit> {
    match init {
        Some(ForStatementInit::VariableDeclaration(v)) => {
            let vars: Vec<(String, Option<hir::Expr>)> = v
                .declarations
                .iter()
                .filter_map(|d| {
                    let name = match &d.id {
                        BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                        _ => return None,
                    };
                    let init = d.init.as_ref().and_then(|e| convert_expr(e).ok());
                    Some((name, init))
                })
                .collect();
            Some(hir::ForInit::Variable(var_kind(v.kind), vars))
        }
        _ => init.as_ref().and_then(|i| {
            i.as_expression().and_then(|e| {
                convert_expr(e).ok().map(|e| hir::ForInit::Expr(Box::new(e)))
            })
        }),
    }
}

fn convert_for_stmt(stmt: &ForStatement) -> hir::Stmt {
    hir::Stmt::For {
        init: convert_for_init_expr(&stmt.init),
        test: stmt.test.as_ref().and_then(|t| convert_expr(t).ok()),
        update: stmt.update.as_ref().and_then(|u| convert_expr(u).ok()),
        body: Box::new(stmt_to_hir_stmt(&stmt.body)),
    }
}

fn convert_for_in_stmt(stmt: &ForInStatement) -> hir::Stmt {
    let left = match &stmt.left {
        ForStatementLeft::VariableDeclaration(v) => {
            let vars: Vec<(String, Option<hir::Expr>)> = v
                .declarations
                .iter()
                .filter_map(|d| {
                    let name = match &d.id {
                        BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                        _ => return None,
                    };
                    Some((name, d.init.as_ref().and_then(|e| convert_expr(e).ok())))
                })
                .collect();
            hir::ForInit::Variable(var_kind(v.kind), vars)
        }
        _ => convert_for_left_expr(&stmt.left),
    };
    hir::Stmt::ForIn {
        left,
        right: convert_expr(&stmt.right).unwrap_or(hir::Expr::Undefined),
        body: Box::new(stmt_to_hir_stmt(&stmt.body)),
    }
}

fn convert_for_of_stmt(stmt: &ForOfStatement) -> hir::Stmt {
    let left = match &stmt.left {
        ForStatementLeft::VariableDeclaration(v) => {
            let vars: Vec<(String, Option<hir::Expr>)> = v
                .declarations
                .iter()
                .filter_map(|d| {
                    let name = match &d.id {
                        BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                        _ => return None,
                    };
                    Some((name, d.init.as_ref().and_then(|e| convert_expr(e).ok())))
                })
                .collect();
            hir::ForInit::Variable(var_kind(v.kind), vars)
        }
        _ => convert_for_left_expr(&stmt.left),
    };
    hir::Stmt::ForOf {
        left,
        right: convert_expr(&stmt.right).unwrap_or(hir::Expr::Undefined),
        body: Box::new(stmt_to_hir_stmt(&stmt.body)),
        is_await: stmt.r#await,
    }
}

fn convert_for_left_expr(left: &ForStatementLeft) -> hir::ForInit {
    match left {
        ForStatementLeft::AssignmentTargetIdentifier(id) => {
            hir::ForInit::Expr(Box::new(hir::Expr::Ident { name: id.name.to_string() }))
        }
        ForStatementLeft::ComputedMemberExpression(m) => {
            hir::ForInit::Expr(Box::new(hir::Expr::Member {
                obj: Box::new(convert_expr(&m.object).unwrap_or(hir::Expr::Undefined)),
                property: Box::new(convert_expr(&m.expression).unwrap_or(hir::Expr::Undefined)),
                computed: true,
            }))
        }
        ForStatementLeft::StaticMemberExpression(m) => {
            hir::ForInit::Expr(Box::new(hir::Expr::Member {
                obj: Box::new(convert_expr(&m.object).unwrap_or(hir::Expr::Undefined)),
                property: Box::new(hir::Expr::Ident { name: m.property.name.to_string() }),
                computed: false,
            }))
        }
        ForStatementLeft::PrivateFieldExpression(m) => {
            hir::ForInit::Expr(Box::new(hir::Expr::Member {
                obj: Box::new(convert_expr(&m.object).unwrap_or(hir::Expr::Undefined)),
                property: Box::new(hir::Expr::Ident { name: m.field.name.to_string() }),
                computed: false,
            }))
        }
        _ => hir::ForInit::Variable(hir::VariableKind::Let, vec![]),
    }
}

fn convert_switch_stmt(stmt: &SwitchStatement) -> hir::Stmt {
    let discriminant = convert_expr(&stmt.discriminant).unwrap_or(hir::Expr::Undefined);
    let cases = stmt.cases.iter().map(|c| {
        hir::SwitchCase {
            test: c.test.as_ref().map(|t| convert_expr(t).unwrap_or(hir::Expr::Undefined)),
            consequent: c.consequent.iter().map(stmt_to_hir_stmt).collect(),
        }
    }).collect();
    hir::Stmt::Switch { discriminant, cases }
}

fn convert_try_stmt(stmt: &TryStatement) -> hir::Stmt {
    let handler = stmt.handler.as_ref().map(|h| {
        hir::CatchClause {
            param: h.param.as_ref().map(|p| {
                match &p.pattern {
                    BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                    _ => String::new(),
                }
            }).unwrap_or_default(),
            body: Box::new(hir::Block(h.body.body.iter().map(stmt_to_hir_stmt).collect())),
        }
    });
    hir::Stmt::Try {
        block: hir::Block(stmt.block.body.iter().map(stmt_to_hir_stmt).collect()),
        handler,
        finalizer: stmt.finalizer.as_ref().map(|f| hir::Block(f.body.iter().map(stmt_to_hir_stmt).collect())),
    }
}

fn convert_var_stmt(v: &VariableDeclaration) -> hir::Stmt {
    let kind = var_kind(v.kind);
    let mut stmts = vec![];
    for decl in &v.declarations {
        let init = decl.init.as_ref().and_then(|e| convert_expr(e).ok());
        match &decl.id {
            BindingPattern::BindingIdentifier(i) => {
                stmts.push(hir::Stmt::Variable(hir::VariableDecl {
                    name: i.name.to_string(),
                    kind: kind.clone(),
                    type_: None,
                    init,
                    pattern: None,
                }));
            }
            _ => {
                if let Some(pat) = convert_binding_pattern(&decl.id) {
                    stmts.push(hir::Stmt::Variable(hir::VariableDecl {
                        name: String::new(),
                        kind: kind.clone(),
                        type_: None,
                        init,
                        pattern: Some(pat),
                    }));
                }
            }
        }
    }
    hir::Stmt::Block { stmts }
}

fn convert_class_decl_stmt(c: &Class) -> hir::Stmt {
    if let hir::Decl::Class(class_decl) = class_to_hir(c) {
        hir::Stmt::Class(class_decl)
    } else {
        hir::Stmt::Empty
    }
}

fn convert_func_decl_stmt(f: &Function) -> hir::Stmt {
    if let hir::Decl::Function(func_decl) = func_to_decl(f) {
        hir::Stmt::FunctionDecl(func_decl)
    } else {
        hir::Stmt::Empty
    }
}

pub fn class_to_hir(c: &Class) -> hir::Decl {
    let mut members: Vec<hir::ClassMember> = Vec::new();
    let mut methods: Vec<hir::ClassMethod> = Vec::new();

    for m in &c.body.body {
        match m {
            ClassElement::MethodDefinition(def) => {
                if let Some(method) = convert_method_def(def) {
                    methods.push(method);
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(member) = convert_prop_def(prop) {
                    members.push(member);
                }
            }
            _ => {}
        }
    }
    hir::Decl::Class(hir::ClassDecl {
        name: c.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        extends: None,
        members,
        generics: vec![],
        methods,
    })
}

fn convert_method_def(def: &MethodDefinition) -> Option<hir::ClassMethod> {
    let name = match &def.key {
        PropertyKey::StaticIdentifier(i) => i.name.to_string(),
        PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
        _ => return None,
    };
    let func = &*def.value;
    let body = extract_method_body(func);
    let params = convert_func_params(func);
    let kind = if name == "constructor" { hir::MethodKind::Constructor } else { hir::MethodKind::Method };
    Some(hir::ClassMethod { name, params, body, kind })
}

fn extract_method_body(func: &Function) -> hir::Expr {
    if let Some(body_box) = &func.body {
        if let Some(stmt) = body_box.statements.first() {
            return match stmt {
                Statement::ExpressionStatement(e) => convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined),
                Statement::ReturnStatement(r) => r.argument.as_ref().and_then(|a| convert_expr(a).ok()).unwrap_or(hir::Expr::Undefined),
                _ => hir::Expr::Undefined,
            };
        }
    }
    hir::Expr::Undefined
}

fn convert_func_params(func: &Function) -> Vec<hir::Param> {
    func.params.items.iter().filter_map(|param| {
        if let BindingPattern::BindingIdentifier(i) = &param.pattern {
            Some(hir::Param {
                name: i.name.to_string(),
                type_: None,
                default: None,
                optional: param.optional,
                pattern: None,
                ownership: hir::Ownership::Owned,
            })
        } else {
            convert_binding_pattern(&param.pattern).map(|pattern| hir::Param {
                name: String::new(),
                type_: None,
                default: None,
                optional: param.optional,
                pattern: Some(pattern),
                ownership: hir::Ownership::Owned,
            })
        }
    }).collect()
}

fn convert_prop_def(prop: &PropertyDefinition) -> Option<hir::ClassMember> {
    let name = match &prop.key {
        PropertyKey::StaticIdentifier(i) => i.name.to_string(),
        PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
        PropertyKey::StringLiteral(s) => s.value.to_string(),
        PropertyKey::NumericLiteral(n) => n.value.to_string(),
        _ => return None,
    };
    Some(hir::ClassMember { name, type_: None, is_static: prop.r#static, is_async: false })
}

// Re-export for use by stmt_decl
pub use super::stmt_decl::func_to_decl;
