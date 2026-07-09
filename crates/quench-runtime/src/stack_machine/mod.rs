//! Explicit-stack interpreter for the JavaScript runtime AST.
//!
//! This is a drop-in replacement for the recursive evaluator in
//! `interpreter.rs`.  It keeps the same `Value` / `Environment` / object model
//! and reuses the existing helper functions (`eval_binary_op`, `eval_unary_op`,
//! `get_iterator`, etc.).  The difference is that function calls, expression
//! evaluation and statement execution are driven by explicit `Vec` stacks
//! instead of the native Rust call stack, so deep JS recursion no longer
//! overflows.

mod calls;
mod eval;
mod eval_helpers;
mod frame;
mod loops;
mod property;
mod statements;
mod work;

use std::rc::Rc;

use self::eval::eval_expr;
use std::cell::RefCell;

use crate::ast::*;
use crate::value::{Value, JsError, to_js_string};
use crate::env::Environment;
use crate::interpreter as hir;

pub use frame::{Machine, Frame, CatchFrame};
pub use work::Work;


/// Evaluate a complete program with hoisting.
pub fn eval_program(program: &Program, env: &mut Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    match program {
        Program::Script(statements) => {
            hir::hoist_functions(statements, env);
            hir::predeclare_let_const(statements, &mut env.borrow_mut());
            let global_this = env.borrow().get("globalThis").unwrap_or(Value::Undefined);
            hir::set_this_binding(env, global_this);
            Machine::new(Rc::clone(env)).run_statements(&Rc::new(statements.clone()))
        }
    }
}

impl Machine {
    /// Run a top-level statement list to completion and return the last value.
    pub fn run_statements(mut self, stmts: &Rc<Vec<Statement>>) -> Result<Value, JsError> {
        self.push_stmt_list(stmts, false);
        self.run()
    }

    /// Run the machine until the frame stack is empty.
    pub fn run(&mut self) -> Result<Value, JsError> {
        loop {
            let work = {
                let frame = match self.frames.last_mut() {
                    Some(f) => f,
                    None => break,
                };
                match frame.work.pop() {
                    Some(w) => w,
                    None => {
                        let result = frame.values.pop().unwrap_or(Value::Undefined);
                        let _ = frame;
                        self.frames.pop();
                        if let Some(caller) = self.frames.last_mut() {
                            caller.values.push(result);
                        } else {
                            return Ok(result);
                        }
                        continue;
                    }
                }
            };

            if let Err(e) = self.step(work) {
                let value = e;
                self.frames.pop();
                while let Some(caller) = self.frames.last_mut() {
                    if let Some(catch_frame) = caller.catches.pop() {
                        caller.work.clear();
                        caller.values.clear();
                        if let Some(name) = catch_frame.param {
                            catch_frame.env.borrow_mut().define(name, Value::String(value.to_string()));
                        }
                        caller.work.push(Work::EvalStmt(Rc::clone(&catch_frame.handler), catch_frame.is_expr_body));
                        break;
                    }
                    caller.values.clear();
                    caller.work.clear();
                    self.frames.pop();
                }
                if self.frames.is_empty() {
                    return Err(value);
                }
            }
        }
        Ok(Value::Undefined)
    }

    pub fn current_frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("no active frame")
    }

    fn push_stmt_list(&mut self, stmts: &[Statement], is_expr_body: bool) {
        if !stmts.is_empty() {
            self.current_frame().work.push(Work::EvalStmts(Rc::new(stmts.to_owned()), is_expr_body, 0));
        } else {
            self.current_frame().values.push(Value::Undefined);
        }
    }

    fn pop_value(&mut self) -> Value {
        self.current_frame().values.pop().unwrap_or(Value::Undefined)
    }

    fn step(&mut self, work: Work) -> Result<(), JsError> {
        use self::calls::*;
        use self::eval_helpers::*;
        use self::property::*;
        use self::statements::*;
        use self::loops::*;

        match work {
            Work::PushValue(v) => self.current_frame().values.push(v),
            Work::EvalExpr(expr) => eval_expr(self, expr)?,
            Work::EvalStmt(stmt, is_expr_body) => eval_stmt(self, stmt, is_expr_body)?,
            Work::EvalStmts(stmts, is_expr_body, index) => eval_stmts(self, &stmts, is_expr_body, index)?,
            Work::ApplyBinary(op) => apply_binary(self, op)?,
            Work::ApplyUnary(op) => apply_unary(self, op)?,
            Work::ApplyAssign { target } => apply_assign(self, target)?,
            Work::ApplyMemberAssign => apply_member_assign(self)?,
            Work::ApplyCompoundAssign { op, target } => apply_compound_assign(self, op, target)?,
            Work::EvalCallee(callee) => eval_callee(self, callee)?,
            Work::ApplyCall { argc } => apply_call(self, argc)?,
            Work::ApplyMember { property, computed, callee_mode } => apply_member(self, property, computed, callee_mode)?,
            Work::ApplyConditional { consequent, alternate } => {
                apply_conditional(self, consequent, alternate)?
            }
            Work::ApplyUpdate { op, prefix, target } => apply_update(self, op, prefix, target)?,
            Work::ApplyNew { argc } => apply_new(self, argc)?,
            Work::ApplyConstructorResult { new_obj, use_constructor_result } => {
                apply_constructor_result(self, new_obj, use_constructor_result)?
            }
            Work::ApplySequence { exprs, index } => apply_sequence(self, &exprs, index)?,
            Work::ApplyBlockExpr { stmts, index } => apply_block_expr(self, &stmts, index)?,
            Work::ApplyIf { consequent, alternate, is_expr_body } => apply_if(self, consequent, alternate, is_expr_body)?,
            Work::ApplyWhile { condition, body, is_expr_body } => apply_while(self, condition, body, is_expr_body)?,
            Work::ApplyWhileBody { condition, body, is_expr_body } => apply_while_body(self, condition, body, is_expr_body)?,
            Work::ApplyFor { condition, update, body, is_expr_body, phase } => {
                apply_for(self, condition, update, body, is_expr_body, phase)?
            }
            Work::ApplyForBody { condition, update, body, is_expr_body } => {
                apply_for_body(self, condition, update, body, is_expr_body)?
            }
            Work::ApplyBlock { stmts, index, is_expr_body } => apply_block(self, &stmts, index, is_expr_body)?,
            Work::ApplyTryCatch { handler, param, is_expr_body } => apply_try_catch(self, handler, param, is_expr_body)?,
            Work::ApplyReturn => apply_return(self)?,
            Work::ApplyForOf { variable, body, items, index } => apply_for_of(self, variable, body, items, index)?,
            Work::ApplyForIn { variable, body, keys, index } => apply_for_in(self, variable, body, keys, index)?,
            Work::ApplyObjectProperty { key, kind, obj } => apply_object_property(self, key, kind, obj)?,
            Work::Discard => { self.pop_value(); }
            Work::VarDecl { kind, name } => var_decl(self, kind, name)?,
            Work::ForInitVar { kind, name } => for_init_var(self, kind, name)?,
            Work::BeginForOf { variable, body } => begin_for_of(self, variable, body)?,
            Work::BeginForIn { variable, body } => begin_for_in(self, variable, body)?,
            Work::PushCatch { handler, param, env, is_expr_body } => {
                self.current_frame().catches.push(CatchFrame { handler, param, env, is_expr_body });
            }
            Work::PopCatch => { self.current_frame().catches.pop(); }
            Work::PopScope => { self.current_frame().env.borrow_mut().pop_scope(); }
            Work::Throw => {
                let msg = to_js_string(&self.pop_value());
                return Err(JsError(msg));
            }
        }
        Ok(())
    }
}

/// Loop phases for for-loop state machine.
#[derive(Debug, Clone, Copy)]
pub enum ForPhase {
    Init,
    Check,
    Update,
}

/// Kind of object property being defined.
#[derive(Debug, Clone, Copy)]
pub enum ObjectPropertyKind {
    Value,
    Getter,
    Setter,
}

/// Assignment target for compound assignments.
#[derive(Debug, Clone)]
pub enum AssignmentTarget {
    Identifier(String),
    #[allow(dead_code)]
    Member { obj: Rc<RefCell<crate::value::Object>>, key: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_eval(src: &str) -> Result<Value, JsError> {
        let ctx = crate::Context::new().unwrap();
        let program = ctx.parse(src)?;
        let mut env = Rc::clone(ctx.env());
        eval_program(&program, &mut env)
    }

    #[test]
    fn test_deep_recursion() {
        // Simple test first
        let result = ctx_eval("5");
        assert!(result.is_ok(), "simple number failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::Number(5.0));

        // Test function declaration and call in same program
        let result = ctx_eval("function foo() { return 42; } foo()");
        assert!(result.is_ok(), "function call failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::Number(42.0));

        // Test function with arguments
        let result = ctx_eval("function add(a, b) { return a + b; } add(2, 3)");
        assert!(result.is_ok(), "function with args failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::Number(5.0));

        // Test recursion
        let result = ctx_eval(r#"
            function recurse(n) {
                if (n <= 0) return 0;
                return 1 + recurse(n - 1);
            }
            recurse(100);
        "#);
        assert!(result.is_ok(), "deep recursion failed: {:?}", result);
        assert_eq!(result.unwrap(), Value::Number(100.0));
    }
}
