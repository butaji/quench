//! Statement generation

use anyhow::Result;
use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct CodeGenStmt;

impl CodeGenStmt {
    pub fn stmt_to_rust(cg: &mut CodeGenerator, stmt: &Stmt) -> Result<String> {
        match stmt {
            Stmt::Empty => Ok(String::new()),
            Stmt::Expr { expr } => Ok(format!("{};", cg.expr_to_rust(expr))),
            Stmt::Return { arg } => {
                Ok(arg.as_ref().map(|e| format!("return {};", cg.expr_to_rust(e))).unwrap_or_else(|| "return;".to_string()))
            }
            Stmt::Block(stmts) => Self::block_to_rust(cg, stmts),
            Stmt::If { test, consequent, alternate } => Self::if_to_rust(cg, test, consequent, alternate),
            Stmt::While { test, body } => Self::while_to_rust(cg, test, body),
            Stmt::DoWhile { body, test } => Self::do_while_to_rust(cg, body, test),
            Stmt::For { init, test, update, body } => Self::for_to_rust(cg, init, test, update, body),
            Stmt::ForIn { left, right, body } => Self::for_in_to_rust(cg, left, right, body),
            Stmt::ForOf { left, right, body, .. } => Self::for_of_to_rust(cg, left, right, body),
            Stmt::Switch { discriminant, cases } => Self::switch_to_rust(cg, discriminant, cases),
            Stmt::Break { label } => Ok(if label.is_some() { format!("break {};", label.as_ref().unwrap()) } else { "break;".to_string() }),
            Stmt::Continue { label } => Ok(if label.is_some() { format!("continue {};", label.as_ref().unwrap()) } else { "continue;".to_string() }),
            Stmt::Try { block, handler, finalizer } => Self::try_to_rust(cg, block, handler, finalizer),
            Stmt::Throw { arg } => Ok(format!("panic!(\"{}\");", cg.expr_to_rust(arg).replace('"', "\\\""))),
            Stmt::Label { label, body } => Ok(format!("'{}: {{ {} }}", label, cg.stmt_to_rust(body)?)),
            Stmt::Function { decl } => Ok(FnGen::generate_function(cg, decl, false)?),
            Stmt::Variable { decl } => Ok(VarGen::generate_variable(cg, decl)?),
            Stmt::Debugger => Ok(String::new()),
            Stmt::With { .. } => Ok(String::new()),
            Stmt::Class { .. } => Ok(String::new()),
        }
    }

    fn block_to_rust(cg: &mut CodeGenerator, stmts: &[Stmt]) -> Result<String> {
        let mut output = String::from("{\n");
        cg.indent += 1;
        for stmt in stmts {
            let code = cg.stmt_to_rust(stmt)?;
            if !code.trim().is_empty() {
                output.push_str(&format!("{}{}\n", cg.indent_str(), code.trim()));
            }
        }
        cg.indent -= 1;
        output.push_str(&format!("{}}}", cg.indent_str()));
        Ok(output)
    }

    fn if_to_rust(cg: &mut CodeGenerator, test: &Expr, consequent: &Stmt, alternate: &Option<Box<Stmt>>) -> Result<String> {
        let test_str = cg.expr_to_rust(test);
        let cons_str = cg.stmt_to_rust(consequent)?;
        let cons_block = if matches!(consequent, Stmt::Block(_)) {
            cons_str
        } else {
            format!("{{ {} }}", cons_str.trim())
        };
        let _alt_str = match alternate {
            Some(a) => {
                let alt_code = cg.stmt_to_rust(a)?;
                if matches!(a.as_ref(), Stmt::Block(_)) {
                    format!(" else {}", alt_code)
                } else {
                    format!(" else {{ {} }}", alt_code.trim())
                }
            }
            None => String::new(),
        };
        Ok(format!("if {} {}", test_str, cons_block))
    }

    fn while_to_rust(cg: &mut CodeGenerator, test: &Expr, body: &Stmt) -> Result<String> {
        let test_str = cg.expr_to_rust(test);
        let body_str = cg.stmt_to_rust(body)?;
        Ok(format!("while {} {}", test_str, body_str))
    }

    fn do_while_to_rust(cg: &mut CodeGenerator, body: &Stmt, test: &Expr) -> Result<String> {
        let body_str = cg.stmt_to_rust(body)?;
        let test_str = cg.expr_to_rust(test);
        Ok(format!("loop {{ {} if !{} {{ break; }} }}", body_str.trim(), test_str))
    }

    fn for_to_rust(cg: &mut CodeGenerator, init: &Option<ForInit>, test: &Option<Expr>, update: &Option<Expr>, body: &Stmt) -> Result<String> {
        let init_str = Self::for_init_to_rust(cg, init);
        let test_str = test.as_ref().map(|e| cg.expr_to_rust(e)).unwrap_or_default();
        let update_str = update.as_ref().map(|e| cg.expr_to_rust(e)).unwrap_or_default();
        let body_str = cg.stmt_to_rust(body)?;
        Ok(format!("for {}; {}; {} {}", init_str, test_str, update_str, body_str))
    }

    fn for_in_to_rust(cg: &mut CodeGenerator, left: &ForInit, right: &Expr, body: &Stmt) -> Result<String> {
        let left_str = Self::for_init_to_rust(cg, &Some(left.clone()));
        let right_str = cg.expr_to_rust(right);
        let body_str = cg.stmt_to_rust(body)?;
        Ok(format!("for {} in &{} {}", left_str, right_str, body_str))
    }

    fn for_of_to_rust(cg: &mut CodeGenerator, left: &ForInit, right: &Expr, body: &Stmt) -> Result<String> {
        let left_str = Self::for_init_to_rust(cg, &Some(left.clone()));
        let right_str = cg.expr_to_rust(right);
        let body_str = cg.stmt_to_rust(body)?;
        Ok(format!("for {} in {} {}", left_str, right_str, body_str))
    }

    fn switch_to_rust(cg: &mut CodeGenerator, discriminant: &Expr, cases: &[SwitchCase]) -> Result<String> {
        let disc_str = cg.expr_to_rust(discriminant);
        let mut output = format!("match {} {{\n", disc_str);
        for case in cases {
            let test_str = case.test.as_ref().map(|t| format!("{:?}", t)).unwrap_or_else(|| "_".to_string());
            cg.indent += 1;
            output.push_str(&format!("{}{} => {{\n", cg.indent_str(), test_str));
            for stmt in &case.consequent {
                let code = cg.stmt_to_rust(stmt)?;
                if !code.trim().is_empty() {
                    output.push_str(&format!("{}{}\n", cg.indent_str(), code.trim()));
                }
            }
            output.push_str(&format!("{}}}\n", cg.indent_str()));
            cg.indent -= 1;
        }
        output.push('}');
        Ok(output)
    }

    fn try_to_rust(cg: &mut CodeGenerator, block: &Box<Stmt>, handler: &Option<Box<Stmt>>, finalizer: &Option<Box<Stmt>>) -> Result<String> {
        let body_str = cg.stmt_to_rust(block)?;
        let catch_str = match handler {
            Some(h) => {
                let h_stmt = cg.stmt_to_rust(h)?;
                format!(" catch {{ {} }}", h_stmt.trim())
            }
            None => String::new(),
        };
        let finally_str = match finalizer {
            Some(f) => {
                let finally_body = cg.stmt_to_rust(f)?;
                format!(" finally {{ {} }}", finally_body.trim())
            }
            None => String::new(),
        };
        Ok(format!("{{ {} {}{} }}", body_str.trim(), catch_str, finally_str))
    }

    pub fn for_init_to_rust(cg: &CodeGenerator, init: &Option<ForInit>) -> String {
        match init {
            Some(ForInit::Variable(decl)) => VarGen::generate_variable(cg, decl).unwrap_or_default().replace('\n', " "),
            Some(ForInit::Expr(e)) => cg.expr_to_rust(e),

            None => String::new(),
        }
    }
}

use crate::transpile::codegen::function::FnGen;
use crate::transpile::codegen::variable::VarGen;
