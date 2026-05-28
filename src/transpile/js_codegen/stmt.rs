//! Statement to JS

use super::super::hir::*;
use super::expr::expr_to_js;

pub fn stmt_to_js(stmt: &Stmt) -> String {
    use Stmt::*;
    match stmt {
        Variable { decl } => { let k = match decl.kind { VariableKind::Const | VariableKind::Let => "const", VariableKind::Var => "var" }; let i = decl.init.as_ref().map_or(String::new(), |v| format!(" = {}", expr_to_js(v))); format!("{} {}{};", k, decl.name, i) }
        Expr { expr } => { let e = expr_to_js(expr); if e.is_empty() { String::new() } else { format!("{};", e) } }
        Return { arg } => arg.as_ref().map_or("return null;".to_string(), |e| format!("return {};", expr_to_js(e))),
        Block(stmts) => { let inner = stmts.0.iter().map(stmt_to_js).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"); format!("{{\n{}\n}}", indent(&inner)) }
        If { test, consequent, alternate } => { let mut s = format!("if ({}) {{", expr_to_js(test)); let then_js = stmt_to_js(consequent); if !then_js.is_empty() { s.push_str(&format!("\n{}\n", indent(&then_js))); } s.push('}'); if let Some(else_stmt) = alternate { let else_js = stmt_to_js(else_stmt); s.push_str(" else {"); if !else_js.is_empty() { s.push_str(&format!("\n{}\n", indent(&else_js))); } s.push('}'); } s }
        While { test, body: _body } => format!("while ({}) {{", expr_to_js(test)),
        For { init, test, update, body: _body } => { let i = init.as_ref().map_or(String::new(), |x| match x { ForInit::Variable(v) => v.init.as_ref().map_or(format!("const {}", v.name), |e| format!("const {} = {}", v.name, expr_to_js(e))), ForInit::Expr(e) => expr_to_js(e) }); let t = test.as_ref().map_or(String::new(), expr_to_js); let u = update.as_ref().map_or(String::new(), expr_to_js); format!("for ({}; {}; {}) {{", i, t, u) }
        Switch { discriminant, cases } => { let mut s = format!("switch ({}) {{\n", expr_to_js(discriminant)); for case in cases { if let Some(test_expr) = &case.test { s.push_str(&format!("  case {}:\n", expr_to_js(test_expr))); } else { s.push_str("  default:\n"); } for stmt in &case.consequent { let stmt_js = stmt_to_js(stmt); if !stmt_js.is_empty() { s.push_str(&format!("    {}\n", stmt_js)); } } } s.push('}'); s }
        Try { block, handler, finalizer } => { let mut s = "try {\n".to_string(); if let Stmt::Block(stmts) = block.as_ref() { for stmt in stmts { let stmt_js = stmt_to_js(stmt); if !stmt_js.is_empty() { s.push_str(&format!("  {}\n", stmt_js)); } } } s.push('}'); if let Some(h) = handler { s.push_str(" catch (e) {\n"); if let Stmt::Block(stmts) = h.as_ref() { for stmt in stmts { let stmt_js = stmt_to_js(stmt); if !stmt_js.is_empty() { s.push_str(&format!("  {}\n", stmt_js)); } } } s.push('}'); } if let Some(finally_block) = finalizer { s.push_str(" finally {\n"); if let Stmt::Block(stmts) = finally_block.as_ref() { for stmt in stmts { let stmt_js = stmt_to_js(stmt); if !stmt_js.is_empty() { s.push_str(&format!("  {}\n", stmt_js)); } } } s.push('}'); } s }
        Break => "break;".to_string(),
        Continue => "continue;".to_string(),
        _ => String::new(),
    }
}

fn indent(s: &str) -> String { s.lines().map(|l| format!("  {}", l)).collect::<Vec<_>>().join("\n") }
