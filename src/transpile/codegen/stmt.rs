//! Statement generation

use anyhow::Result;
use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenStmt;

impl CodeGenStmt {
    pub fn stmt_to_rust(cg: &mut CodeGenerator, stmt: &Stmt) -> Result<String> {
        use Stmt::*;
        match stmt {
            Empty => Ok(String::new()),
            Expr { expr } => Ok(format!("{};", cg.expr_to_rust(expr))),
            Return { arg } => Ok(arg.as_ref().map_or("return;".to_string(), |e| format!("return {};", cg.expr_to_rust(e)))),
            Block(stmts) => Self::block_to_rust(cg, stmts),
            If { test, consequent, alternate } => Self::if_to_rust(cg, test, consequent, alternate),
            While { test, body } => Self::while_to_rust(cg, test, body),
            DoWhile { body, test } => Self::do_while_to_rust(cg, body, test),
            For { init, test, update, body } => Self::for_to_rust(cg, init, test, update, body),
            ForIn { left, right, body } => Self::for_in_to_rust(cg, left, right, body),
            ForOf { left, right, body, .. } => Self::for_of_to_rust(cg, left, right, body),
            Switch { discriminant, cases } => Self::switch_to_rust(cg, discriminant, cases),
            Break { label } => Ok(if label.is_some() { format!("break {};", label.as_ref().unwrap()) } else { "break;".to_string() }),
            Continue { label } => Ok(if label.is_some() { format!("continue {};", label.as_ref().unwrap()) } else { "continue;".to_string() }),
            Try { block, handler, finalizer } => Self::try_to_rust(cg, block, handler, finalizer),
            Throw { arg } => Ok(format!("panic!(\"{}\");", cg.expr_to_rust(arg).replace('"', "\\\""))),
            Label { label, body } => Ok(format!("'{}: {{ {} }}", label, cg.stmt_to_rust(body)?)),
            Function { decl } => FnGen::generate_function(cg, decl, false),
            Variable { decl } => VarGen::generate_variable(cg, decl),
            Debugger | With { .. } | Class { .. } => Ok(String::new()),
        }
    }

    fn block_to_rust(cg: &mut CodeGenerator, stmts: &[Stmt]) -> Result<String> {
        let mut output = String::from("{\n"); cg.indent += 1;
        for stmt in stmts { let code = cg.stmt_to_rust(stmt)?; if !code.trim().is_empty() { output.push_str(&format!("{}{}\n", cg.indent_str(), code.trim())); } }
        cg.indent -= 1; output.push_str(&format!("{}}}", cg.indent_str())); Ok(output)
    }

    fn if_to_rust(cg: &mut CodeGenerator, test: &Expr, consequent: &Stmt, alternate: &Option<Box<Stmt>>) -> Result<String> {
        let t = cg.expr_to_rust(test); let c = cg.stmt_to_rust(consequent)?;
        match alternate { Some(a) => Ok(format!("if {} {{ {} }} else {{ {} }}", t, c, cg.stmt_to_rust(a)?)), None => Ok(format!("if {} {{ {} }}", t, c)) }
    }

    fn while_to_rust(cg: &mut CodeGenerator, test: &Expr, body: &Stmt) -> Result<String> { Ok(format!("while {} {{ {} }}", cg.expr_to_rust(test), cg.stmt_to_rust(body)?)) }
    fn do_while_to_rust(cg: &mut CodeGenerator, body: &Stmt, test: &Expr) -> Result<String> { Ok(format!("loop {{ {} if !({}) {{ break; }} }}", cg.stmt_to_rust(body)?, cg.expr_to_rust(test))) }

    fn for_to_rust(cg: &mut CodeGenerator, init: &Option<ForInit>, test: &Option<Expr>, update: &Option<Expr>, body: &Stmt) -> Result<String> {
        let i = init.as_ref().map_or(String::new(), |x| Self::for_init_to_str(cg, x));
        let t = test.as_ref().map_or(String::new(), |x| cg.expr_to_rust(x));
        let u = update.as_ref().map_or(String::new(), |x| cg.expr_to_rust(x));
        Ok(format!("for {}; {}; {} {{ {} }}", i, t, u, cg.stmt_to_rust(body)?))
    }

    fn for_init_to_str(cg: &CodeGenerator, init: &ForInit) -> String { match init { ForInit::Variable(v) => format!("let {} = {}", v.name, cg.expr_to_rust(&v.init.as_ref().unwrap_or(&Expr::Null))), ForInit::Expr(e) => cg.expr_to_rust(e), ForInit::Pat(p) => cg.expr_to_rust(&Expr::Ident { name: p.clone() }) } }

    fn for_in_to_rust(cg: &mut CodeGenerator, left: &ForInit, right: &Expr, body: &Stmt) -> Result<String> { Ok(format!("for {} in {} {{ {} }}", Self::for_init_to_str(cg, left), cg.expr_to_rust(right), cg.stmt_to_rust(body)?)) }
    fn for_of_to_rust(cg: &mut CodeGenerator, left: &ForInit, right: &Expr, body: &Stmt) -> Result<String> { Ok(format!("for {} in {} {{ {} }}", Self::for_init_to_str(cg, left), cg.expr_to_rust(right), cg.stmt_to_rust(body)?)) }

    fn switch_to_rust(cg: &mut CodeGenerator, discriminant: &Expr, cases: &[SwitchCase]) -> Result<String> {
        let mut output = format!("match {} {{\n", cg.expr_to_rust(discriminant));
        for case in cases { let t = case.test.as_ref().map_or("_".to_string(), |e| cg.expr_to_rust(e)); output.push_str(&format!("{} => {{ {} }}\n", t, case.consequent.iter().map(|s| cg.stmt_to_rust(s)).collect::<Result<Vec<_>>>()?.join("; "))); }
        output.push_str("}"); Ok(output)
    }

    fn try_to_rust(cg: &mut CodeGenerator, block: &Stmt, handler: &Option<Box<Stmt>>, finalizer: &Option<Box<Stmt>>) -> Result<String> {
        let b = cg.stmt_to_rust(block)?; let h = handler.as_ref().map_or(String::new(), |x| cg.stmt_to_rust(x).unwrap_or_default()); let f = finalizer.as_ref().map_or(String::new(), |x| cg.stmt_to_rust(x).unwrap_or_default());
        Ok(format!("let result = (|| {{ {} }})(); if let Err(e) = result {{ {} }} {}", b, h, f))
    }
}
