use super::super::hir::*;

pub fn stmts_have_call(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| stmt_has_call(s, name))
}

pub fn stmt_has_call(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Expr(e) => expr_has_call(&e.expr, name),
        Stmt::If(i) => {
            stmts_have_call(&i.consequent, name) 
            || i.alternative.as_ref().map_or(false, |a| stmts_have_call(a, name))
        }
        Stmt::While(w) => stmts_have_call(&w.body, name),
        Stmt::ForLoop(f) => stmts_have_call(&f.body, name),
        Stmt::ForIn(f) => stmts_have_call(&f.body, name),
        Stmt::ForOf(f) => stmts_have_call(&f.body, name),
        Stmt::Return(r) => r.arg.as_ref().map_or(false, |e| expr_has_call(e, name)),
        Stmt::Block(b) => stmts_have_call(&b.0, name),
        _ => false,
    }
}

pub fn expr_has_call(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Call(c) => {
            match &c.callee {
                Callee::Expr(callee) => {
                    if let Expr::Ident(i) = callee.as_ref() {
                        i.name == name
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }
        Expr::JSX(j) => jsx_has_call(j, name),
        _ => false,
    }
}

pub fn jsx_has_call(jsx: &JSXExpr, name: &str) -> bool {
    jsx.children.iter().any(|c| {
        if let JSXChild::Expr(e) = c {
            expr_has_call(e, name)
        } else {
            false
        }
    })
}

pub fn has_hook_calls(stmts: &[Stmt]) -> bool {
    stmts_have_call(stmts, "useState")
}
