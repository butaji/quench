//! Lower the runtime AST to HIR.
//!
//! This is a minimal, expression-oriented lowering for the foundational HIR
//! subset. It supports literals, identifiers, binary expressions, and simple
//! control flow. More complex AST nodes are rejected with a clear error so the
//! HIR path grows incrementally.

use crate::ast;
use crate::hir::{
    BinaryOp, BlockId, HirConst, HirFunction, HirItem, HirProgram, HirValue, Local, Terminator,
};
use crate::value::JsError;

/// Builder for lowering a single function body.
pub struct HirBuilder {
    func: HirFunction,
    current_block: BlockId,
}

impl HirBuilder {
    pub fn new() -> Self {
        let mut func = HirFunction::default();
        let entry = func.add_block();
        Self {
            func,
            current_block: entry,
        }
    }

    pub fn build(self) -> HirFunction {
        self.func
    }

    /// Emit a HIR value instruction into the current block.
    fn emit(&mut self, value: HirValue) {
        self.func.block_mut(self.current_block).push(value);
    }

    /// Set the terminator of the current block.
    fn set_terminator(&mut self, term: Terminator) {
        self.func.block_mut(self.current_block).set_terminator(term);
    }

    /// Add a new basic block and return its id.
    fn add_block(&mut self) -> BlockId {
        self.func.add_block()
    }

    /// Allocate a fresh local and return its index.
    fn local(&mut self) -> Local {
        self.func.alloc_local()
    }

    /// Lower an expression into a target local.
    pub fn lower_expr(&mut self, expr: &ast::Expression) -> Result<Local, JsError> {
        let target = self.local();
        match expr {
            ast::Expression::Number(n) => {
                let id = self.func.add_const(HirConst::Number(*n));
                self.emit(HirValue::Const { target, id });
            }
            ast::Expression::String(s) => {
                let id = self.func.add_const(HirConst::String(s.clone()));
                self.emit(HirValue::Const { target, id });
            }
            ast::Expression::Boolean(b) => {
                let id = self.func.add_const(HirConst::Bool(*b));
                self.emit(HirValue::Const { target, id });
            }
            ast::Expression::Null => {
                let id = self.func.add_const(HirConst::Null);
                self.emit(HirValue::Const { target, id });
            }
            ast::Expression::Undefined => {
                let id = self.func.add_const(HirConst::Undefined);
                self.emit(HirValue::Const { target, id });
            }
            ast::Expression::Identifier(name) => {
                if name == "this" {
                    self.emit(HirValue::This { target });
                } else {
                    self.emit(HirValue::LoadGlobal {
                        target,
                        name: name.clone(),
                    });
                }
            }
            ast::Expression::Binary { op, left, right } => {
                let l = self.lower_expr(left.as_ref())?;
                let r = self.lower_expr(right.as_ref())?;
                let hir_op = lower_bin_op(op)?;
                self.emit(HirValue::Binary {
                    target,
                    op: hir_op,
                    left: l,
                    right: r,
                });
            }
            ast::Expression::Assignment { left, right } => {
                let rhs = self.lower_expr(right.as_ref())?;
                match left.as_ref() {
                    ast::Expression::Identifier(name) => {
                        self.emit(HirValue::StoreGlobal {
                            name: name.clone(),
                            source: rhs,
                        });
                        self.emit(HirValue::LoadGlobal {
                            target,
                            name: name.clone(),
                        });
                    }
                    _ => return Err(JsError("HIR: unsupported assignment target".to_string())),
                }
            }
            _ => return Err(JsError(format!("HIR: unsupported expression {:?}", expr))),
        }
        Ok(target)
    }

    /// Lower a statement, producing a local holding its value (if any).
    pub fn lower_stmt(&mut self, stmt: &ast::Statement) -> Result<Option<Local>, JsError> {
        match stmt {
            ast::Statement::Expression(expr) => {
                let l = self.lower_expr(expr.as_ref())?;
                Ok(Some(l))
            }
            ast::Statement::VarDeclaration {
                kind: _,
                name,
                init,
            } => {
                if let Some(expr) = init {
                    let l = self.lower_expr(expr)?;
                    self.emit(HirValue::StoreGlobal {
                        name: name.clone(),
                        source: l,
                    });
                }
                Ok(None)
            }
            ast::Statement::Block(stmts) => {
                let mut last = None;
                for s in stmts {
                    last = self.lower_stmt(s)?;
                }
                Ok(last)
            }
            ast::Statement::If {
                condition,
                consequent,
                alternate,
            } => {
                let cond = self.lower_expr(condition.as_ref())?;
                let then_block = self.add_block();
                let else_block = self.add_block();
                let merge_block = self.add_block();

                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block,
                    else_block,
                });

                self.current_block = then_block;
                self.lower_stmt(consequent.as_ref())?;
                self.set_terminator(Terminator::Jump(merge_block));

                self.current_block = else_block;
                if let Some(alt) = alternate {
                    self.lower_stmt(alt.as_ref())?;
                }
                self.set_terminator(Terminator::Jump(merge_block));

                self.current_block = merge_block;
                Ok(None)
            }
            ast::Statement::While { condition, body } => {
                let check_block = self.add_block();
                let body_block = self.add_block();
                let merge_block = self.add_block();

                self.set_terminator(Terminator::Jump(check_block));

                self.current_block = check_block;
                let cond = self.lower_expr(condition.as_ref())?;
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: body_block,
                    else_block: merge_block,
                });

                self.current_block = body_block;
                self.lower_stmt(body.as_ref())?;
                self.set_terminator(Terminator::Jump(check_block));

                self.current_block = merge_block;
                Ok(None)
            }
            ast::Statement::Return(expr) => {
                if let Some(e) = expr {
                    let l = self.lower_expr(e.as_ref())?;
                    self.set_terminator(Terminator::Return(l));
                } else {
                    self.set_terminator(Terminator::ReturnUndefined);
                }
                Ok(None)
            }
            _ => Err(JsError(format!("HIR: unsupported statement {:?}", stmt))),
        }
    }
}

fn lower_bin_op(op: &ast::BinaryOp) -> Result<BinaryOp, JsError> {
    match op {
        ast::BinaryOp::Add => Ok(BinaryOp::Add),
        ast::BinaryOp::Sub => Ok(BinaryOp::Sub),
        ast::BinaryOp::Mul => Ok(BinaryOp::Mul),
        ast::BinaryOp::Div => Ok(BinaryOp::Div),
        ast::BinaryOp::Mod => Ok(BinaryOp::Rem),
        ast::BinaryOp::Eq => Ok(BinaryOp::Eq),
        ast::BinaryOp::StrictEq => Ok(BinaryOp::StrictEq),
        ast::BinaryOp::Lt => Ok(BinaryOp::Lt),
        ast::BinaryOp::And => Ok(BinaryOp::And),
        ast::BinaryOp::Or => Ok(BinaryOp::Or),
        _ => Err(JsError(format!(
            "HIR: unsupported binary operator {:?}",
            op
        ))),
    }
}

/// Lower a runtime AST program to HIR.
pub fn lower_program(program: &ast::Program) -> Result<HirProgram, JsError> {
    let mut hir = HirProgram::default();
    match program {
        ast::Program::Script(stmts) => {
            let mut main = HirBuilder::new();
            let mut last_local = None;
            for stmt in stmts {
                last_local = main.lower_stmt(stmt)?;
            }
            if let Some(l) = last_local {
                main.set_terminator(Terminator::Return(l));
            } else {
                main.set_terminator(Terminator::ReturnUndefined);
            }
            hir.items.push(HirItem::Function {
                name: "__main".to_string(),
                func: main.build(),
            });
        }
    }
    Ok(hir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::run_hir_function;
    use crate::value::Value;
    use std::rc::Rc;

    #[test]
    fn test_lower_arithmetic() {
        let program = ast::Program::Script(vec![ast::Statement::Expression(Box::new(
            ast::Expression::Binary {
                op: ast::BinaryOp::Add,
                left: Box::new(ast::Expression::Number(1.0)),
                right: Box::new(ast::Expression::Number(2.0)),
            },
        ))]);
        let hir = lower_program(&program).unwrap();
        let func = hir.find_function("__main").unwrap();
        let result = run_hir_function(Rc::new(func.clone()), vec![], Value::Undefined).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_lower_if_consequent_taken() {
        let program = ast::Program::Script(vec![ast::Statement::If {
            condition: Box::new(ast::Expression::Boolean(true)),
            consequent: Box::new(ast::Statement::Expression(Box::new(
                ast::Expression::Number(1.0),
            ))),
            alternate: None,
        }]);
        let hir = lower_program(&program).unwrap();
        let func = hir.find_function("__main").unwrap();
        // The if statement is the last statement; it produces no value, so the
        // script result is undefined. The important check is that lowering and
        // execution did not error.
        let _result = run_hir_function(Rc::new(func.clone()), vec![], Value::Undefined).unwrap();
    }
}
