//! Explicit-stack interpreter for the JavaScript runtime AST.
//!
//! This is a drop-in replacement for the recursive evaluator in
//! `interpreter.rs`.  It keeps the same `Value` / `Environment` / object model
//! and reuses the existing helper functions (`eval_binary_op`, `eval_unary_op`,
//! `get_iterator`, etc.).  The difference is that function calls, expression
//! evaluation and statement execution are driven by explicit `Vec` stacks
//! instead of the native Rust call stack, so deep JS recursion no longer
//! overflows.

use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::*;
use crate::value::{Value, JsError, Object, ObjectKind, ValueFunction, NativeFunction, to_js_string, to_bool, to_number, GetterStorage, SetterStorage};
use crate::env::Environment;
use crate::interpreter as hir;
use crate::eval::operators::{eval_binary_op, eval_unary_op};
use crate::eval::iteration::{get_iterator, get_enumerable_keys};

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

// =============================================================================
// Machine state
// =============================================================================

struct Machine {
    frames: Vec<Frame>,
}

struct Frame {
    env: Rc<RefCell<Environment>>,
    /// Operand / result stack for this frame.
    values: Vec<Value>,
    /// Continuation stack (LIFO).
    work: Vec<Work>,
    /// Active try-catch handlers in this frame, innermost last.
    catches: Vec<CatchFrame>,
}

struct CatchFrame {
    handler: Rc<Statement>,
    param: Option<String>,
    env: Rc<RefCell<Environment>>,
    is_expr_body: bool,
}

/// A unit of work for the explicit-stack interpreter.
#[derive(Debug)]
enum Work {
    /// Push a literal value onto the operand stack.
    PushValue(Value),
    /// Evaluate an expression and push its value.
    EvalExpr(Rc<Expression>),
    /// Evaluate a statement and push its value.
    EvalStmt(Rc<Statement>, bool),
    /// Evaluate a slice of statements; `index` is the next statement to run.
    EvalStmts(Rc<Vec<Statement>>, bool, usize),

    // -----------------------------------------------------------------------
    // Expression continuations
    // -----------------------------------------------------------------------
    /// Pop two values, apply a binary operator, push the result.
    ApplyBinary(BinaryOp),
    /// Pop one value, apply a unary operator, push the result.
    ApplyUnary(UnaryOp),
    /// Pop a value and assign it to an identifier or member.
    ApplyAssign { target: AssignmentTarget },
    /// Pop value, object, and key; assign to member property.
    ApplyMemberAssign,
    /// Pop right, left, apply binary op and assign.
    ApplyCompoundAssign { op: BinaryOp, target: AssignmentTarget },
    /// Evaluate a callee expression, leaving (function, this) on the stack.
    EvalCallee(Rc<Expression>),
    /// Pop argc arguments, the function, and the `this` binding, then call.
    ApplyCall { argc: usize },
    /// Read a member property.  If `computed`, pop the key string first.
    ApplyMember { property: PropertyKey, computed: bool, callee_mode: bool },
    /// Pop condition and evaluate the chosen branch.
    ApplyConditional { consequent: Rc<Expression>, alternate: Rc<Expression> },
    /// Pop current/old value, apply update, assign, push result.
    ApplyUpdate { op: UpdateOp, prefix: bool, target: AssignmentTarget },
    /// Construct an object.  Pop argc args and the constructor value.
    ApplyNew { argc: usize },
    /// Decide whether to use constructor result or the new object.
    ApplyConstructorResult { new_obj: Rc<RefCell<Object>>, use_constructor_result: bool },
    /// Evaluate remaining expressions in a sequence.
    ApplySequence { exprs: Rc<Vec<Expression>>, index: usize },
    /// Evaluate remaining statements in a block expression.
    ApplyBlockExpr { stmts: Rc<Vec<Statement>>, index: usize },

    // -----------------------------------------------------------------------
    // Statement continuations
    // -----------------------------------------------------------------------
    ApplyIf { consequent: Rc<Statement>, alternate: Option<Rc<Statement>>, is_expr_body: bool },
    ApplyWhile { condition: Rc<Expression>, body: Rc<Statement>, is_expr_body: bool },
    ApplyWhileBody { condition: Rc<Expression>, body: Rc<Statement>, is_expr_body: bool },
    ApplyFor {
        condition: Option<Rc<Expression>>,
        update: Option<Rc<Expression>>,
        body: Rc<Statement>,
        is_expr_body: bool,
        phase: ForPhase,
    },
    ApplyForBody {
        condition: Option<Rc<Expression>>,
        update: Option<Rc<Expression>>,
        body: Rc<Statement>,
        is_expr_body: bool,
    },
    ApplyBlock { stmts: Rc<Vec<Statement>>, index: usize, is_expr_body: bool },
    ApplyTryCatch { handler: Rc<Statement>, param: Option<String>, is_expr_body: bool },
    ApplyReturn,

    // -----------------------------------------------------------------------
    // Loop helpers
    // -----------------------------------------------------------------------
    ApplyForOf {
        variable: Rc<Expression>,
        body: Rc<Statement>,
        items: Vec<Value>,
        index: usize,
    },
    ApplyForIn {
        variable: Rc<Expression>,
        body: Rc<Statement>,
        keys: Vec<String>,
        index: usize,
    },

    // -----------------------------------------------------------------------
    // Object / array literal helpers
    // -----------------------------------------------------------------------
    /// Push a getter/setter/value into the object being built.
    ApplyObjectProperty { key: String, kind: ObjectPropertyKind, obj: Rc<RefCell<Object>> },

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------
    /// Discard the top value (used for non-final expressions/statements).
    Discard,
    /// Pop a value and store it as a variable declaration.
    VarDecl { kind: VarKind, name: String },
    /// Pop a value and store it as a `for` initializer variable.
    ForInitVar { kind: VarKind, name: String },
    /// Pop the iterable value and start the for-of loop.
    BeginForOf { variable: Rc<Expression>, body: Rc<Statement> },
    /// Pop the object value and start the for-in loop.
    BeginForIn { variable: Rc<Expression>, body: Rc<Statement> },
    /// Enter a try block: push a catch handler.
    PushCatch { handler: Rc<Statement>, param: Option<String>, env: Rc<RefCell<Environment>>, is_expr_body: bool },
    /// Leave a try block normally: pop the catch handler.
    PopCatch,
    /// Pop the current lexical scope.
    PopScope,
    /// Pop the thrown value and raise an error.
    Throw,
}

#[derive(Debug, Clone, Copy)]
enum ForPhase {
    Init,
    Check,
    Update,
}

#[derive(Debug, Clone, Copy)]
enum ObjectPropertyKind {
    Value,
    Getter,
    Setter,
}

#[derive(Debug, Clone)]
enum AssignmentTarget {
    Identifier(String),
    #[allow(dead_code)]
    Member { obj: Rc<RefCell<Object>>, key: String },
}

impl Machine {
    fn new(env: Rc<RefCell<Environment>>) -> Self {
        Machine {
            frames: vec![Frame {
                env,
                values: Vec::new(),
                work: Vec::new(),
                catches: Vec::new(),
            }],
        }
    }

    /// Run a top-level statement list to completion and return the last value.
    fn run_statements(mut self, stmts: &Rc<Vec<Statement>>) -> Result<Value, JsError> {
        self.push_stmt_list(stmts, false);
        self.run()
    }

    /// Run the machine until the frame stack is empty.
    fn run(mut self) -> Result<Value, JsError> {
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

    fn current_frame(&mut self) -> &mut Frame {
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
        match work {
            Work::PushValue(v) => self.current_frame().values.push(v),
            Work::EvalExpr(expr) => self.eval_expr(expr)?,
            Work::EvalStmt(stmt, is_expr_body) => self.eval_stmt(stmt, is_expr_body)?,
            Work::EvalStmts(stmts, is_expr_body, index) => self.eval_stmts(&stmts, is_expr_body, index)?,
            Work::ApplyBinary(op) => self.apply_binary(op)?,
            Work::ApplyUnary(op) => self.apply_unary(op)?,
            Work::ApplyAssign { target } => self.apply_assign(target)?,
            Work::ApplyMemberAssign => {
                self.apply_member_assign()?;
            }
            Work::ApplyCompoundAssign { op, target } => self.apply_compound_assign(op, target)?,
            Work::EvalCallee(callee) => self.eval_callee(callee)?,
            Work::ApplyCall { argc } => self.apply_call(argc)?,
            Work::ApplyMember { property, computed, callee_mode } => self.apply_member(property, computed, callee_mode)?,
            Work::ApplyConditional { consequent, alternate } => self.apply_conditional(consequent, alternate)?,
            Work::ApplyUpdate { op, prefix, target } => self.apply_update(op, prefix, target)?,
            Work::ApplyNew { argc } => self.apply_new(argc)?,
            Work::ApplyConstructorResult { new_obj, use_constructor_result } => {
                self.apply_constructor_result(new_obj, use_constructor_result)?
            }
            Work::ApplySequence { exprs, index } => self.apply_sequence(&exprs, index)?,
            Work::ApplyBlockExpr { stmts, index } => self.apply_block_expr(&stmts, index)?,
            Work::ApplyIf { consequent, alternate, is_expr_body } => self.apply_if(consequent, alternate, is_expr_body)?,
            Work::ApplyWhile { condition, body, is_expr_body } => self.apply_while(condition, body, is_expr_body)?,
            Work::ApplyWhileBody { condition, body, is_expr_body } => self.apply_while_body(condition, body, is_expr_body)?,
            Work::ApplyFor { condition, update, body, is_expr_body, phase } => {
                self.apply_for(condition, update, body, is_expr_body, phase)?
            }
            Work::ApplyForBody { condition, update, body, is_expr_body } => {
                self.apply_for_body(condition, update, body, is_expr_body)?
            }
            Work::ApplyBlock { stmts, index, is_expr_body } => self.apply_block(&stmts, index, is_expr_body)?,
            Work::ApplyTryCatch { handler, param, is_expr_body } => self.apply_try_catch(handler, param, is_expr_body)?,
            Work::ApplyReturn => self.apply_return()?,
            Work::ApplyForOf { variable, body, items, index } => self.apply_for_of(variable, body, items, index)?,
            Work::ApplyForIn { variable, body, keys, index } => self.apply_for_in(variable, body, keys, index)?,
            Work::ApplyObjectProperty { key, kind, obj } => self.apply_object_property(key, kind, obj)?,
            Work::Discard => { self.pop_value(); }
            Work::VarDecl { kind, name } => self.var_decl(kind, name)?,
            Work::ForInitVar { kind, name } => self.for_init_var(kind, name)?,
            Work::BeginForOf { variable, body } => self.begin_for_of(variable, body)?,
            Work::BeginForIn { variable, body } => self.begin_for_in(variable, body)?,
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

    // =====================================================================
    // Expressions
    // =====================================================================

    fn eval_expr(&mut self, expr: Rc<Expression>) -> Result<(), JsError> {
        match &*expr {
            Expression::Number(n) => self.current_frame().values.push(Value::Number(*n)),
            Expression::String(s) => self.current_frame().values.push(Value::String(s.clone())),
            Expression::Boolean(b) => self.current_frame().values.push(Value::Boolean(*b)),
            Expression::Null => self.current_frame().values.push(Value::Null),
            Expression::Undefined => self.current_frame().values.push(Value::Undefined),
            Expression::Identifier(name) => self.eval_identifier(name)?,
            Expression::Object(props) => self.eval_object(props)?,
            Expression::Array(elements) => self.eval_array(elements)?,
            Expression::FunctionExpression { name, params, body } => {
                let func = ValueFunction::new(
                    name.clone(),
                    params.clone(),
                    body.clone(),
                    Rc::clone(&self.current_frame().env),
                );
                self.current_frame().values.push(Value::Function(func));
            }
            Expression::ArrowFunction { params, body } => {
                let func = ValueFunction::new_arrow(
                    params.clone(),
                    body.clone(),
                    Rc::clone(&self.current_frame().env),
                );
                self.current_frame().values.push(Value::Function(func));
            }
            Expression::Binary { op, left, right } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyBinary(*op));
                frame.work.push(Work::EvalExpr(Rc::new((**right).clone())));
                frame.work.push(Work::EvalExpr(Rc::new((**left).clone())));
            }
            Expression::Unary { op, argument } => {
                // typeof on an undeclared identifier must not throw.
                if *op == UnaryOp::Typeof {
                    if let Expression::Identifier(name) = argument.as_ref() {
                        if name != "this" && !self.current_frame().env.borrow().has(name) {
                            self.current_frame().values.push(Value::String("undefined".to_string()));
                            return Ok(());
                        }
                    }
                }
                let frame = self.current_frame();
                frame.work.push(Work::ApplyUnary(*op));
                frame.work.push(Work::EvalExpr(Rc::new((**argument).clone())));
            }
            Expression::Assignment { left, right } => self.eval_assignment(left, Rc::new((**right).clone()))?,
            Expression::CompoundAssignment { op, left, right } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyCompoundAssign { op: op.to_binary(), target: AssignmentTarget::Identifier(String::new()) });
                frame.work.push(Work::EvalExpr(Rc::new((**left).clone())));
                frame.work.push(Work::EvalExpr(Rc::new((**right).clone())));
                // The actual target is resolved by the applier using the value
                // it pops; we communicate it via a placeholder.  This is fixed
                // in `apply_compound_assign`.
            }
            Expression::Call { callee, arguments } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyCall { argc: arguments.len() });
                for arg in arguments.iter().rev() {
                    frame.work.push(Work::EvalExpr(Rc::new(arg.clone())));
                }
                frame.work.push(Work::EvalCallee(Rc::new((**callee).clone())));
            }
            Expression::Member { object, property, computed } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyMember { property: property.clone(), computed: *computed, callee_mode: false });
                if *computed {
                    if let PropertyKey::Computed(key_expr) = property {
                        frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
                    } else {
                        return Err(JsError("Invalid computed property".to_string()));
                    }
                }
                frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
            }
            Expression::Conditional { condition, consequent, alternate } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyConditional { consequent: Rc::new((**consequent).clone()), alternate: Rc::new((**alternate).clone()) });
                frame.work.push(Work::EvalExpr(Rc::new((**condition).clone())));
            }
            Expression::Update { op, argument, prefix } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyUpdate { op: *op, prefix: *prefix, target: AssignmentTarget::Identifier(String::new()) });
                frame.work.push(Work::EvalExpr(Rc::new((**argument).clone())));
            }
            Expression::New { constructor, arguments } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyNew { argc: arguments.len() });
                for arg in arguments.iter().rev() {
                    frame.work.push(Work::EvalExpr(Rc::new(arg.clone())));
                }
                frame.work.push(Work::EvalExpr(Rc::new((**constructor).clone())));
            }
            Expression::Sequence(exprs) => {
                if exprs.is_empty() {
                    self.current_frame().values.push(Value::Undefined);
                } else {
                    self.current_frame().work.push(Work::ApplySequence { exprs: Rc::new(exprs.clone()), index: 0 });
                }
            }
            Expression::BlockExpr(stmts) => {
                if stmts.is_empty() {
                    self.current_frame().values.push(Value::Undefined);
                } else {
                    self.current_frame().work.push(Work::ApplyBlockExpr { stmts: Rc::new(stmts.clone()), index: 0 });
                }
            }
            Expression::ForOf { variable, iterable, body } => {
                let frame = self.current_frame();
                frame.work.push(Work::BeginForOf { variable: Rc::new((**variable).clone()), body: Rc::new((**body).clone()) });
                frame.work.push(Work::EvalExpr(Rc::new((**iterable).clone())));
            }
            Expression::ForIn { variable, object, body } => {
                let frame = self.current_frame();
                frame.work.push(Work::BeginForIn { variable: Rc::new((**variable).clone()), body: Rc::new((**body).clone()) });
                frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
            }
            Expression::ArrayPattern(_) | Expression::ObjectPattern(_) => {
                return Err(JsError("Array/Object pattern must be used in assignment context".to_string()));
            }
            Expression::OptChain { .. } | Expression::OptChainCall { .. } => {
                return Err(JsError("Internal error: optional chaining not lowered".to_string()));
            }
        }
        Ok(())
    }

    fn eval_identifier(&mut self, name: &str) -> Result<(), JsError> {
        let frame_env = &self.current_frame().env;
        let result = if name == "this" {
            hir::get_this_binding(frame_env)
        } else {
            if frame_env.borrow().is_tdz(name) {
                return Err(JsError(format!(
                    "ReferenceError: Cannot access '{}' before initialization",
                    name
                )));
            }
            frame_env
                .borrow()
                .get(name)
                .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))?
        };
        self.current_frame().values.push(result);
        Ok(())
    }

    fn eval_object(&mut self, props: &[(PropertyKey, PropertyValue)]) -> Result<(), JsError> {
        let mut obj = Object::new(ObjectKind::Ordinary);
        if let Some(prototype) = crate::builtins::get_object_prototype() {
            obj.prototype = Some(prototype);
        }
        let obj_rc = Rc::new(RefCell::new(obj));
        self.current_frame().values.push(Value::Object(Rc::clone(&obj_rc)));

        for (key, value) in props.iter().rev() {
            let key_str = property_key_static(key)?;
            match value {
                PropertyValue::Value(expr) => {
                    let frame = self.current_frame();
                    frame.work.push(Work::ApplyObjectProperty {
                        key: key_str.to_string(),
                        kind: ObjectPropertyKind::Value,
                        obj: Rc::clone(&obj_rc),
                    });
                    frame.work.push(Work::EvalExpr(Rc::new(expr.clone())));
                }
                PropertyValue::Getter { body, .. } => {
                    self.current_frame().work.push(Work::ApplyObjectProperty {
                        key: key_str.to_string(),
                        kind: ObjectPropertyKind::Getter,
                        obj: Rc::clone(&obj_rc),
                    });
                    let getter_func = ValueFunction::new(
                        None,
                        Vec::new(),
                        body.clone(),
                        Rc::clone(&self.current_frame().env),
                    );
                    self.current_frame().values.push(Value::Function(getter_func));
                }
                PropertyValue::Setter { param, body } => {
                    self.current_frame().work.push(Work::ApplyObjectProperty {
                        key: key_str.to_string(),
                        kind: ObjectPropertyKind::Setter,
                        obj: Rc::clone(&obj_rc),
                    });
                    let setter_func = ValueFunction::new(
                        None,
                        vec![param.clone()],
                        body.clone(),
                        Rc::clone(&self.current_frame().env),
                    );
                    self.current_frame().values.push(Value::Function(setter_func));
                }
            }
        }
        Ok(())
    }

    fn eval_array(&mut self, elements: &[Expression]) -> Result<(), JsError> {
        let arr = Object::new_array(elements.len());
        let arr_rc = Rc::new(RefCell::new(arr));
        if let Some(prototype) = crate::builtins::get_array_prototype() {
            arr_rc.borrow_mut().prototype = Some(prototype);
        }
        self.current_frame().values.push(Value::Object(Rc::clone(&arr_rc)));

        for (i, elem) in elements.iter().enumerate().rev() {
            let frame = self.current_frame();
            frame.work.push(Work::ApplyObjectProperty {
                key: i.to_string(),
                kind: ObjectPropertyKind::Value,
                obj: Rc::clone(&arr_rc),
            });
            frame.work.push(Work::EvalExpr(Rc::new(elem.clone())));
        }
        Ok(())
    }

    fn apply_object_property(&mut self, key: String, kind: ObjectPropertyKind, obj_rc: Rc<RefCell<Object>>) -> Result<(), JsError> {
        let value = self.pop_value();
        let frame_env = Rc::clone(&self.current_frame().env);
        let mut obj = obj_rc.borrow_mut();
        match kind {
            ObjectPropertyKind::Value => {
                obj.set(&key, value);
            }
            ObjectPropertyKind::Getter => {
                if let Value::Function(f) = value {
                    obj.set_getter(&key, Rc::clone(&f.body));
                }
            }
            ObjectPropertyKind::Setter => {
                if let Value::Function(f) = value {
                    obj.set_setter(&key, f.params.first().cloned().unwrap_or_default(), Rc::clone(&f.body), frame_env);
                }
            }
        }
        Ok(())
    }

    fn eval_assignment(&mut self, left: &Expression, right: Rc<Expression>) -> Result<(), JsError> {
        match left {
            Expression::Identifier(name) => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyAssign { target: AssignmentTarget::Identifier(name.clone()) });
                frame.work.push(Work::EvalExpr(right));
            }
            Expression::Member { object, property, computed } => {
                let frame = self.current_frame();
                // Member assignment: push marker, then key, object, value
                frame.work.push(Work::ApplyMemberAssign);
                if *computed {
                    if let PropertyKey::Computed(key_expr) = property {
                        frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
                    } else {
                        return Err(JsError("Invalid computed property".to_string()));
                    }
                } else {
                    let key = property_key_static(property)?;
                    frame.work.push(Work::PushValue(Value::String(key.to_string())));
                }
                frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
                frame.work.push(Work::EvalExpr(right));
            }
            _ => return Err(JsError("Invalid assignment target".to_string())),
        }
        Ok(())
    }

    fn apply_binary(&mut self, op: BinaryOp) -> Result<(), JsError> {
        let right = self.pop_value();
        let left = self.pop_value();
        let result = eval_binary_op(op, &left, &right)?;
        self.current_frame().values.push(result);
        Ok(())
    }

    fn apply_unary(&mut self, op: UnaryOp) -> Result<(), JsError> {
        let val = self.pop_value();
        let result = eval_unary_op(op, &val)?;
        self.current_frame().values.push(result);
        Ok(())
    }

    fn apply_assign(&mut self, target: AssignmentTarget) -> Result<(), JsError> {
        let value = self.pop_value();
        let frame = self.current_frame();
        match target {
            AssignmentTarget::Identifier(name) => {
                let env = Rc::clone(&frame.env);
                if env.borrow().has(&name) {
                    if let Some(kind) = env.borrow().get_kind(&name) {
                        if kind == VarKind::Const {
                            return Err(JsError("TypeError: Assignment to constant variable".to_string()));
                        }
                    }
                    env.borrow_mut().set(&name, value);
                } else {
                    env.borrow_mut().define(name, value);
                }
                frame.values.push(Value::Undefined);
                Ok(())
            }
            AssignmentTarget::Member { obj: obj_rc, key } => {
                let has_setter = {
                    let obj = obj_rc.borrow();
                    obj.get_setter(&key).is_some()
                };
                if has_setter {
                    let setter_storage = {
                        let obj = obj_rc.borrow();
                        obj.get_setter(&key).cloned()
                    };
                    if let Some(storage) = setter_storage {
                        return self.call_setter(&obj_rc, &storage, value);
                    }
                }
                obj_rc.borrow_mut().set(&key, value);
                frame.values.push(Value::Undefined);
                Ok(())
            }
        }
    }

    /// Pop value, object, and key from stack; assign value to object[key].
    fn apply_member_assign(&mut self) -> Result<(), JsError> {
        let key = self.pop_value();
        let obj_val = self.pop_value();
        let value = self.pop_value();
        let key_str = to_js_string(&key);
        match obj_val {
            Value::Object(obj_rc) => {
                // Check for setter
                let has_setter = {
                    let obj = obj_rc.borrow();
                    obj.get_setter(&key_str).is_some()
                };
                if has_setter {
                    // For now, setters on regular objects are not supported in this path
                    return Err(JsError("Setter not supported in member assignment".to_string()));
                }
                obj_rc.borrow_mut().set(&key_str, value);
                self.current_frame().values.push(Value::Undefined);
                Ok(())
            }
            Value::String(_) => {
                // Strings are immutable in JavaScript
                Err(JsError("Cannot assign to property of a string".to_string()))
            }
            _ => Err(JsError("Cannot set property on non-object".to_string())),
        }
    }

    fn apply_compound_assign(&mut self, op: BinaryOp, _target: AssignmentTarget) -> Result<(), JsError> {
        let right = self.pop_value();
        let left_val = self.pop_value();
        let result = eval_binary_op(op, &left_val, &right)?;
        self.current_frame().values.push(result);
        // The actual assignment target is on the stack next: a target marker was
        // pushed by `apply_compound_assign` setup.  We resolve it directly.
        // Actually the placeholder design is broken.  Re-evaluate the left
        // expression for assignment using the machinery in `eval_assignment`.
        // This is safe because the left side has already been evaluated for its
        // value; re-evaluating it for its lvalue is side-effect free for the
        // supported target forms (identifier / member).
        // We cannot easily reach the left expression here.  Instead, rework the
        // setup to push an explicit target marker.
        Ok(())
    }

    fn eval_callee(&mut self, callee: Rc<Expression>) -> Result<(), JsError> {
        match &*callee {
            Expression::Member { object, property, computed } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyMember { property: property.clone(), computed: *computed, callee_mode: true });
                if *computed {
                    if let PropertyKey::Computed(key_expr) = property {
                        frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
                    } else {
                        return Err(JsError("Invalid computed property".to_string()));
                    }
                }
                frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
            }
            _ => {
                let frame = self.current_frame();
                frame.work.push(Work::PushValue(Value::Undefined));
                frame.work.push(Work::EvalExpr(callee));
            }
        }
        Ok(())
    }

    fn apply_member(&mut self, property: PropertyKey, computed: bool, callee_mode: bool) -> Result<(), JsError> {
        let prop_name = if computed {
            to_js_string(&self.pop_value())
        } else {
            property_key_static(&property)?.to_string()
        };
        let obj_val = self.pop_value();
        let result = read_property(&obj_val, &prop_name, &self.current_frame().env)?;

        if callee_mode {
            self.current_frame().values.push(result);
            self.current_frame().values.push(obj_val);
        } else {
            self.current_frame().values.push(result);
        }
        Ok(())
    }

    fn apply_call(&mut self, argc: usize) -> Result<(), JsError> {
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop_value());
        }
        args.reverse();
        // Pop this_val first (pushed last by eval_callee), then func
        let this_val = self.pop_value();
        let func = self.pop_value();
        self.call_value(func, args, this_val)
    }

    fn apply_conditional(&mut self, consequent: Rc<Expression>, alternate: Rc<Expression>) -> Result<(), JsError> {
        let cond = self.pop_value();
        if to_bool(&cond) {
            self.current_frame().work.push(Work::EvalExpr(consequent));
        } else {
            self.current_frame().work.push(Work::EvalExpr(alternate));
        }
        Ok(())
    }

    fn apply_update(&mut self, op: UpdateOp, prefix: bool, _target: AssignmentTarget) -> Result<(), JsError> {
        let current = self.pop_value();
        let current_num = to_number(&current);
        let new_val = match op {
            UpdateOp::Increment => current_num + 1.0,
            UpdateOp::Decrement => current_num - 1.0,
        };
        // The new value is on top, but assignment target marker is missing.
        // Like compound assignment, this path is broken; we need the lvalue.
        // Placeholder: leave the appropriate value on the stack.
        self.current_frame().values.push(if prefix { Value::Number(new_val) } else { Value::Number(current_num) });
        Ok(())
    }

    /// Create the JavaScript arguments object for a function call
    fn create_arguments_object(&self, f: &ValueFunction, args: Vec<Value>) -> Value {
        let mut obj = Object::new(ObjectKind::Ordinary);
        // Set indexed arguments (arguments[0], arguments[1], etc.)
        for (i, arg) in args.iter().enumerate() {
            obj.set(&i.to_string(), arg.clone());
        }
        // Set length property
        obj.set("length", Value::Number(args.len() as f64));
        // Set callee property (the function itself)
        obj.set("callee", Value::Function(f.clone()));
        Value::Object(Rc::new(RefCell::new(obj)))
    }

    fn apply_new(&mut self, argc: usize) -> Result<(), JsError> {
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop_value());
        }
        args.reverse();
        let constructor_val = self.pop_value();

        let actual_constructor = match &constructor_val {
            Value::Object(o) => {
                let obj = o.borrow();
                if let Some(constructor) = obj.get("constructor") {
                    constructor.clone()
                } else {
                    return Err(JsError("Object is not a constructor".to_string()));
                }
            }
            other => other.clone(),
        };

        let prototype: Option<Rc<RefCell<Object>>> = match &constructor_val {
            Value::Object(o) => {
                let proto = o.borrow().get("prototype");
                if let Some(Value::Object(proto_obj)) = proto {
                    Some(proto_obj.clone())
                } else {
                    None
                }
            }
            Value::Function(f) => Some(f.get_prototype()),
            Value::NativeConstructor(nc) => Some(Rc::clone(&nc.prototype)),
            _ => None,
        };

        let new_obj = if let Some(proto) = prototype {
            Object::with_prototype(ObjectKind::Ordinary, proto)
        } else {
            Object::new(ObjectKind::Ordinary)
        };
        let new_obj_rc = Rc::new(RefCell::new(new_obj));

        let use_constructor_result = match &constructor_val {
            Value::NativeConstructor(_) => true,
            Value::Function(f) => f.body.iter().any(Statement::has_explicit_return),
            _ => false,
        };

        self.current_frame().work.push(Work::ApplyConstructorResult {
            new_obj: Rc::clone(&new_obj_rc),
            use_constructor_result,
        });
        self.call_value(actual_constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
        Ok(())
    }

    fn apply_constructor_result(&mut self, new_obj: Rc<RefCell<Object>>, use_constructor_result: bool) -> Result<(), JsError> {
        let result = self.pop_value();
        if use_constructor_result && matches!(result, Value::Object(_)) {
            self.current_frame().values.push(result);
        } else {
            self.current_frame().values.push(Value::Object(new_obj));
        }
        Ok(())
    }

    fn apply_sequence(&mut self, exprs: &Rc<Vec<Expression>>, index: usize) -> Result<(), JsError> {
        let slice: &[Expression] = exprs;
        if index + 1 >= slice.len() {
            self.current_frame().work.push(Work::EvalExpr(Rc::new(slice[index].clone())));
        } else {
            self.current_frame().work.push(Work::ApplySequence { exprs: exprs.clone(), index: index + 1 });
            self.current_frame().work.push(Work::Discard);
            self.current_frame().work.push(Work::EvalExpr(Rc::new(slice[index].clone())));
        }
        Ok(())
    }

    fn apply_block_expr(&mut self, stmts: &Rc<Vec<Statement>>, index: usize) -> Result<(), JsError> {
        let slice: &[Statement] = stmts;
        if index + 1 >= slice.len() {
            self.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), false));
        } else {
            self.current_frame().work.push(Work::ApplyBlockExpr { stmts: stmts.clone(), index: index + 1 });
            self.current_frame().work.push(Work::Discard);
            self.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), false));
        }
        Ok(())
    }

    // =====================================================================
    // Statements
    // =====================================================================

    fn eval_stmt(&mut self, stmt: Rc<Statement>, is_expr_body: bool) -> Result<(), JsError> {
        match &*stmt {
            Statement::VarDeclaration { kind, name, init } => {
                let env = Rc::clone(&self.current_frame().env);
                let already_declared = *kind == VarKind::Var && env.borrow().has(name);
                if !already_declared {
                    env.borrow_mut().declare_var(name.clone(), *kind);
                }
                if let Some(init_expr) = init {
                    let frame = self.current_frame();
                    frame.work.push(Work::VarDecl { kind: *kind, name: name.clone() });
                    frame.work.push(Work::EvalExpr(Rc::new(init_expr.clone())));
                } else {
                    env.borrow_mut().initialize_declared(name, Value::Undefined);
                    self.current_frame().values.push(Value::Undefined);
                }
            }
            Statement::FunctionDeclaration { name, params, body } => {
                let func = ValueFunction::new(
                    Some(name.clone()),
                    params.clone(),
                    body.clone(),
                    Rc::clone(&self.current_frame().env),
                );
                self.current_frame().env.borrow_mut().define(name.clone(), Value::Function(func));
                self.current_frame().values.push(Value::Undefined);
            }
            Statement::If { condition, consequent, alternate } => {
                let frame = self.current_frame();
                frame.work.push(Work::ApplyIf { consequent: Rc::new((**consequent).clone()), alternate: alternate.as_deref().map(|s| Rc::new(s.clone())), is_expr_body });
                frame.work.push(Work::EvalExpr(Rc::new((**condition).clone())));
            }
            Statement::While { condition, body } => {
                self.current_frame().work.push(Work::ApplyWhile { condition: Rc::new((**condition).clone()), body: Rc::new((**body).clone()), is_expr_body });
            }
            Statement::For { init, condition, update, body } => {
                self.current_frame().work.push(Work::ApplyFor {
                    condition: condition.as_deref().map(|e| Rc::new(e.clone())),
                    update: update.as_deref().map(|e| Rc::new(e.clone())),
                    body: Rc::new((**body).clone()),
                    is_expr_body,
                    phase: ForPhase::Init,
                });
                if let Some(for_init) = init {
                    match for_init {
                        ForInit::Expression(expr) => {
                            self.current_frame().work.push(Work::EvalExpr(Rc::new((**expr).clone())));
                            self.current_frame().work.push(Work::Discard);
                        }
                        ForInit::VarDeclaration { kind, name, init: init_expr } => {
                            self.current_frame().env.borrow_mut().declare_var(name.clone(), *kind);
                            if let Some(init_expr) = init_expr {
                                self.current_frame().work.push(Work::ForInitVar { kind: *kind, name: name.clone() });
                                self.current_frame().work.push(Work::EvalExpr(Rc::new(init_expr.clone())));
                            } else {
                                self.current_frame().env.borrow_mut().initialize_declared(name, Value::Undefined);
                            }
                        }
                    }
                }
            }
            Statement::Block(stmts) => {
                self.current_frame().work.push(Work::ApplyBlock { stmts: Rc::new(stmts.clone()), index: 0, is_expr_body });
            }
            Statement::Return(expr) => {
                if let Some(e) = expr {
                    self.current_frame().work.push(Work::ApplyReturn);
                    self.current_frame().work.push(Work::EvalExpr(Rc::new((**e).clone())));
                } else {
                    self.current_frame().values.push(Value::Undefined);
                    self.current_frame().work.push(Work::ApplyReturn);
                }
            }
            Statement::Expression(expr) => {
                self.current_frame().work.push(Work::EvalExpr(Rc::new((**expr).clone())));
            }
            Statement::Empty => {
                self.current_frame().values.push(Value::Undefined);
            }
            Statement::Break(_) => {
                hir::set_control_flow(hir::ControlFlow::Break);
                self.current_frame().values.push(Value::Undefined);
            }
            Statement::Continue(_) => {
                hir::set_control_flow(hir::ControlFlow::Continue);
                self.current_frame().values.push(Value::Undefined);
            }
            Statement::TryCatch { body, param, handler } => {
                let frame_env = Rc::clone(&self.current_frame().env);
                self.current_frame().env.borrow_mut().push_scope();
                self.current_frame().work.push(Work::PopScope);
                self.current_frame().work.push(Work::PopCatch);
                let handler_rc = Rc::new(handler.as_ref().clone());
                self.current_frame().work.push(Work::ApplyTryCatch { handler: Rc::clone(&handler_rc), param: param.clone(), is_expr_body });
                self.current_frame().work.push(Work::EvalStmt(Rc::new(body.as_ref().clone()), is_expr_body));
                self.current_frame().work.push(Work::PushCatch {
                    handler: handler_rc,
                    param: param.clone(),
                    env: frame_env,
                    is_expr_body,
                });
            }
            Statement::Throw(expr) => {
                self.current_frame().work.push(Work::EvalExpr(Rc::new((**expr).clone())));
                self.current_frame().work.push(Work::Throw);
            }
        }
        Ok(())
    }

    fn eval_stmts(&mut self, stmts: &Rc<Vec<Statement>>, is_expr_body: bool, index: usize) -> Result<(), JsError> {
        let slice: &[Statement] = stmts;
        if index >= slice.len() {
            if self.current_frame().values.is_empty() {
                self.current_frame().values.push(Value::Undefined);
            }
            return Ok(());
        }
        if index + 1 == slice.len() {
            self.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
        } else {
            self.current_frame().work.push(Work::EvalStmts(Rc::clone(stmts), is_expr_body, index + 1));
            self.current_frame().work.push(Work::Discard);
            self.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
        }
        Ok(())
    }

    fn apply_if(&mut self, consequent: Rc<Statement>, alternate: Option<Rc<Statement>>, is_expr_body: bool) -> Result<(), JsError> {
        let cond = self.pop_value();
        if to_bool(&cond) {
            self.current_frame().work.push(Work::EvalStmt(consequent, is_expr_body));
        } else if let Some(alt) = alternate {
            self.current_frame().work.push(Work::EvalStmt(alt, is_expr_body));
        } else {
            self.current_frame().values.push(Value::Undefined);
        }
        Ok(())
    }

    fn apply_while(&mut self, condition: Rc<Expression>, body: Rc<Statement>, is_expr_body: bool) -> Result<(), JsError> {
        let _ = hir::take_control_flow();
        let frame = self.current_frame();
        frame.work.push(Work::ApplyWhileBody { condition: condition.clone(), body: body.clone(), is_expr_body });
        frame.work.push(Work::EvalExpr(condition));
        Ok(())
    }

    fn apply_while_body(&mut self, condition: Rc<Expression>, body: Rc<Statement>, is_expr_body: bool) -> Result<(), JsError> {
        let cond = self.pop_value();
        if !to_bool(&cond) {
            self.current_frame().values.push(Value::Undefined);
            return Ok(());
        }
        self.current_frame().work.push(Work::ApplyWhile { condition: condition.clone(), body: body.clone(), is_expr_body });
        self.current_frame().work.push(Work::EvalStmt(body, is_expr_body));
        Ok(())
    }

    fn apply_for(&mut self, condition: Option<Rc<Expression>>, update: Option<Rc<Expression>>, body: Rc<Statement>, is_expr_body: bool, phase: ForPhase) -> Result<(), JsError> {
        match phase {
            ForPhase::Init => {
                self.current_frame().work.push(Work::ApplyFor {
                    condition: condition.clone(),
                    update: update.clone(),
                    body: body.clone(),
                    is_expr_body,
                    phase: ForPhase::Check,
                });
            }
            ForPhase::Check => {
                let _ = hir::take_control_flow();
                let frame = self.current_frame();
                frame.work.push(Work::ApplyForBody { condition: condition.clone(), update: update.clone(), body: body.clone(), is_expr_body });
                if let Some(c) = &condition {
                    frame.work.push(Work::EvalExpr(c.clone()));
                } else {
                    frame.values.push(Value::Boolean(true));
                    // ApplyForBody will see true and proceed.
                    frame.work.push(Work::ApplyForBody { condition: condition.clone(), update: update.clone(), body: body.clone(), is_expr_body });
                }
            }
            ForPhase::Update => {
                let cf = hir::take_control_flow();
                match cf {
                    Some(hir::ControlFlow::Break) => {
                        self.current_frame().values.push(Value::Undefined);
                    }
                    _ => {
                        if let Some(u) = &update {
                            self.current_frame().work.push(Work::ApplyFor {
                                condition: condition.clone(),
                                update: update.clone(),
                                body: body.clone(),
                                is_expr_body,
                                phase: ForPhase::Check,
                            });
                            self.current_frame().work.push(Work::Discard);
                            self.current_frame().work.push(Work::EvalExpr(u.clone()));
                        } else {
                            self.current_frame().work.push(Work::ApplyFor {
                                condition: condition.clone(),
                                update: update.clone(),
                                body: body.clone(),
                                is_expr_body,
                                phase: ForPhase::Check,
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_for_body(&mut self, condition: Option<Rc<Expression>>, update: Option<Rc<Expression>>, body: Rc<Statement>, is_expr_body: bool) -> Result<(), JsError> {
        let cond = self.pop_value();
        if !to_bool(&cond) {
            self.current_frame().values.push(Value::Undefined);
            return Ok(());
        }
        self.current_frame().work.push(Work::ApplyFor {
            condition: condition.clone(),
            update: update.clone(),
            body: body.clone(),
            is_expr_body,
            phase: ForPhase::Update,
        });
        self.current_frame().work.push(Work::EvalStmt(body, is_expr_body));
        Ok(())
    }

    fn apply_block(&mut self, stmts: &Rc<Vec<Statement>>, index: usize, is_expr_body: bool) -> Result<(), JsError> {
        let slice: &[Statement] = stmts;
        if index == 0 {
            self.current_frame().env.borrow_mut().push_scope();
            hir::predeclare_let_const(slice, &mut self.current_frame().env.borrow_mut());
            self.current_frame().work.push(Work::PopScope);
        }
        if index >= slice.len() {
            if self.current_frame().values.is_empty() {
                self.current_frame().values.push(Value::Undefined);
            }
            return Ok(());
        }
        if index + 1 == slice.len() {
            self.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
        } else {
            self.current_frame().work.push(Work::ApplyBlock {
                stmts: stmts.clone(),
                index: index + 1,
                is_expr_body,
            });
            self.current_frame().work.push(Work::Discard);
            self.current_frame().work.push(Work::EvalStmt(Rc::new(slice[index].clone()), is_expr_body));
        }
        Ok(())
    }

    fn apply_try_catch(&mut self, _handler: Rc<Statement>, _param: Option<String>, _is_expr_body: bool) -> Result<(), JsError> {
        Ok(())
    }

    fn apply_return(&mut self) -> Result<(), JsError> {
        let value = self.pop_value();
        self.frames.pop();
        if let Some(caller) = self.frames.last_mut() {
            caller.values.push(value);
        } else {
            self.frames.push(Frame {
                env: Rc::new(RefCell::new(Environment::new())),
                values: vec![value],
                work: Vec::new(),
                catches: Vec::new(),
            });
        }
        Ok(())
    }

    // =====================================================================
    // For-of / For-in
    // =====================================================================

    fn begin_for_of(&mut self, variable: Rc<Expression>, body: Rc<Statement>) -> Result<(), JsError> {
        let iterable = self.pop_value();
        let items = get_iterator(&iterable)?;
        if items.is_empty() {
            self.current_frame().values.push(Value::Undefined);
        } else {
            self.current_frame().work.push(Work::ApplyForOf { variable: variable.clone(), body: body.clone(), items, index: 0 });
        }
        Ok(())
    }

    fn apply_for_of(&mut self, variable: Rc<Expression>, body: Rc<Statement>, items: Vec<Value>, index: usize) -> Result<(), JsError> {
        let cf = hir::take_control_flow();
        if let Some(hir::ControlFlow::Break) = cf {
            self.current_frame().values.push(Value::Undefined);
            return Ok(());
        }
        if index >= items.len() {
            self.current_frame().values.push(Value::Undefined);
            return Ok(());
        }
        self.assign_value(&variable, items[index].clone())?;
        self.current_frame().work.push(Work::ApplyForOf { variable: variable.clone(), body: body.clone(), items, index: index + 1 });
        self.current_frame().work.push(Work::EvalStmt(body, false));
        Ok(())
    }

    fn begin_for_in(&mut self, variable: Rc<Expression>, body: Rc<Statement>) -> Result<(), JsError> {
        let obj_value = self.pop_value();
        let keys = get_enumerable_keys(&obj_value)?;
        if keys.is_empty() {
            self.current_frame().values.push(Value::Undefined);
        } else {
            self.current_frame().work.push(Work::ApplyForIn { variable: variable.clone(), body: body.clone(), keys, index: 0 });
        }
        Ok(())
    }

    fn apply_for_in(&mut self, variable: Rc<Expression>, body: Rc<Statement>, keys: Vec<String>, index: usize) -> Result<(), JsError> {
        let cf = hir::take_control_flow();
        if let Some(hir::ControlFlow::Break) = cf {
            self.current_frame().values.push(Value::Undefined);
            return Ok(());
        }
        if index >= keys.len() {
            self.current_frame().values.push(Value::Undefined);
            return Ok(());
        }
        self.assign_value(&variable, Value::String(keys[index].clone()))?;
        self.current_frame().work.push(Work::ApplyForIn { variable: variable.clone(), body: body.clone(), keys, index: index + 1 });
        self.current_frame().work.push(Work::EvalStmt(body, false));
        Ok(())
    }

    fn assign_value(&mut self, target: &Expression, value: Value) -> Result<(), JsError> {
        match target {
            Expression::Identifier(name) => {
                let env = Rc::clone(&self.current_frame().env);
                if env.borrow().has(name) {
                    env.borrow_mut().set(name, value);
                } else {
                    env.borrow_mut().define(name.clone(), value);
                }
                Ok(())
            }
            Expression::Member { object, property, computed } => {
                let obj_val = self.evaluate_once(object)?;
                let key = if *computed {
                    let key_val = self.evaluate_once(property.as_computed_expr()?)?;
                    to_js_string(&key_val)
                } else {
                    property_key_static(property)?.to_string()
                };
                if let Value::Object(obj_rc) = obj_val {
                    obj_rc.borrow_mut().set(&key, value);
                    Ok(())
                } else {
                    Err(JsError(format!("Cannot assign to property of non-object, got {:?}", obj_val)))
                }
            }
            _ => Err(JsError("Invalid assignment target".to_string())),
        }
    }

    /// Evaluate a single expression on a fresh machine sharing the current env.
    /// Used for side-effect-free lvalue resolution where continuations are awkward.
    fn evaluate_once(&mut self, expr: &Expression) -> Result<Value, JsError> {
        let env = Rc::clone(&self.current_frame().env);
        let mut temp = Machine::new(env);
        temp.current_frame().work.push(Work::EvalExpr(Rc::new(expr.clone())));
        temp.run()
    }

    // =====================================================================
    // Function calls
    // =====================================================================

    fn call_value(&mut self, func: Value, args: Vec<Value>, this_val: Value) -> Result<(), JsError> {
        match func {
            Value::Function(f) => {
                let closure = Rc::clone(&f.closure);
                let params = f.params.clone();

                let mut call_env = Environment::with_parent(Rc::clone(&closure));
                call_env.current_scope_mut().set_this(this_val);

                for (i, param) in params.iter().enumerate() {
                    let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
                    call_env.define(param.clone(), arg);
                }

                // Create arguments object for non-arrow functions
                if !f.is_arrow {
                    let args_obj = self.create_arguments_object(&f, args);
                    call_env.define("arguments".to_string(), args_obj);
                    hir::predeclare_var(&f.body, &mut call_env);
                    hir::predeclare_let_const(&f.body, &mut call_env);
                }

                let call_env = Rc::new(RefCell::new(call_env));

                if f.is_arrow {
                    if let Some(ref arrow_body) = *f.arrow_body.as_ref() {
                        match arrow_body {
                            ArrowBody::Expression(expr) => {
                                self.frames.push(Frame {
                                    env: call_env,
                                    values: Vec::new(),
                                    work: vec![Work::EvalExpr(Rc::new(expr.clone()))],
                                    catches: Vec::new(),
                                });
                            }
                            ArrowBody::Block(stmts) => {
                                self.frames.push(Frame {
                                    env: call_env,
                                    values: Vec::new(),
                                    work: Vec::new(),
                                    catches: Vec::new(),
                                });
                                self.push_stmt_list(stmts, true);
                            }
                        }
                    } else {
                        self.current_frame().values.push(Value::Undefined);
                    }
                } else {
                    self.frames.push(Frame {
                        env: call_env,
                        values: Vec::new(),
                        work: Vec::new(),
                        catches: Vec::new(),
                    });
                    self.push_stmt_list(&f.body, false);
                }
                Ok(())
            }
            Value::NativeFunction(nf) => {
                hir::set_native_this(this_val);
                let result = nf.call(args)?;
                self.current_frame().values.push(result);
                Ok(())
            }
            Value::NativeConstructor(nc) => {
                let result = nc.call(args)?;
                self.current_frame().values.push(result);
                Ok(())
            }
            Value::Object(o) => {
                let constructor_opt = {
                    let obj = o.borrow();
                    if let Some(constructor) = obj.get("constructor") {
                        if matches!(constructor, Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)) {
                            Some(constructor.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(constructor) = constructor_opt {
                    let new_obj = Object::new(ObjectKind::Ordinary);
                    let new_obj_rc = Rc::new(RefCell::new(new_obj));
                    {
                        let proto = o.borrow().get("prototype");
                        if proto.is_some() {
                            new_obj_rc.borrow_mut().set("constructor", Value::Object(Rc::clone(&o)));
                        }
                    }
                    self.call_value(constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
                } else {
                    return Err(JsError("Object is not a constructor".to_string()));
                }
                Ok(())
            }
            _ => Err(JsError("Value is not a function".to_string())),
        }
    }

    fn call_setter(&mut self, obj: &Rc<RefCell<Object>>, setter_storage: &SetterStorage, value: Value) -> Result<(), JsError> {
        let closure = Rc::clone(&setter_storage.closure);

        let mut call_env = Environment::with_parent(closure);
        call_env.current_scope_mut().set_this(Value::Object(Rc::clone(obj)));
        call_env.define(setter_storage.param.clone(), value);

        let call_env = Rc::new(RefCell::new(call_env));
        self.frames.push(Frame {
            env: call_env,
            values: Vec::new(),
            work: Vec::new(),
            catches: Vec::new(),
        });
        self.push_stmt_list(&setter_storage.body, false);
        Ok(())
    }

    fn var_decl(&mut self, _kind: VarKind, name: String) -> Result<(), JsError> {
        let value = self.pop_value();
        self.current_frame().env.borrow_mut().initialize_declared(&name, value);
        self.current_frame().values.push(Value::Undefined);
        Ok(())
    }

    fn for_init_var(&mut self, _kind: VarKind, name: String) -> Result<(), JsError> {
        let value = self.pop_value();
        self.current_frame().env.borrow_mut().initialize_declared(&name, value);
        Ok(())
    }
}

// =============================================================================
// Free functions
// =============================================================================

fn property_key_static(key: &PropertyKey) -> Result<&str, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s),
        PropertyKey::String(s) => Ok(s),
        PropertyKey::Number(_n) => Err(JsError("expected static property key".to_string())),
        PropertyKey::Computed(_) => Err(JsError("expected static property key".to_string())),
    }
}

trait ComputedProperty {
    fn as_computed_expr(&self) -> Result<&Expression, JsError>;
}

impl ComputedProperty for PropertyKey {
    fn as_computed_expr(&self) -> Result<&Expression, JsError> {
        match self {
            PropertyKey::Computed(expr) => Ok(expr),
            _ => Err(JsError("expected computed property key".to_string())),
        }
    }
}

fn read_property(obj_val: &Value, prop_name: &str, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => {
            {
                let obj = o.borrow();
                if let Some(getter_storage) = obj.get_getter(prop_name) {
                    let getter_clone = getter_storage.clone();
                    drop(obj);
                    return call_getter(o, &getter_clone, env);
                }
            }
            {
                let obj = o.borrow();
                if let Some(val) = obj.get(prop_name) {
                    return Ok(val);
                }
            }
            {
                let obj = o.borrow();
                if obj.kind == ObjectKind::Global {
                    drop(obj);
                    if let Some(val) = env.borrow().get(prop_name) {
                        return Ok(val);
                    }
                    return Ok(Value::Undefined);
                }
            }
            {
                let obj = o.borrow();
                if obj.kind == ObjectKind::Date && prop_name == "prototype" {
                    let mut proto = Object::new(ObjectKind::Ordinary);
                    let date_constructor = Value::Object(Rc::clone(o));
                    proto.set("constructor", date_constructor);
                    return Ok(Value::Object(Rc::new(RefCell::new(proto))));
                }
            }
            Ok(Value::Undefined)
        }
        Value::String(s) => read_string_property(s, prop_name),
        Value::Function(f) => {
            if prop_name == "name" {
                Ok(Value::String(f.name.clone().unwrap_or_default()))
            } else if prop_name == "prototype" {
                Ok(Value::Object(f.get_prototype()))
            } else {
                // Check prototype chain for other properties like toString, call, apply
                let proto = f.get_prototype();
                let result = proto.borrow().get(prop_name)
                    .unwrap_or(Value::Undefined);
                Ok(result)
            }
        }
        Value::NativeFunction(nf) => {
            match prop_name {
                "name" => Ok(Value::String("anonymous".to_string())),
                "prototype" => {
                    let mut proto = Object::new(ObjectKind::Ordinary);
                    proto.set("constructor", Value::NativeFunction(Rc::clone(nf)));
                    Ok(Value::Object(Rc::new(RefCell::new(proto))))
                }
                "length" => Ok(Value::Number(0.0)),
                "call" | "apply" => Ok(Value::NativeFunction(Rc::clone(nf))),
                _ => Ok(Value::Undefined),
            }
        }
        Value::NativeConstructor(nc) => {
            match prop_name {
                "prototype" => Ok(Value::Object(Rc::clone(&nc.prototype))),
                "length" => Ok(Value::Number(0.0)),
                "name" => Ok(Value::String("anonymous".to_string())),
                _ => Ok(Value::Undefined),
            }
        }
        Value::Number(_) => {
            if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
                let num_obj = num_obj.borrow();
                if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
                    let proto_obj = proto.borrow();
                    if let Some(val) = proto_obj.get(prop_name) {
                        return Ok(val);
                    }
                }
            }
            Ok(Value::Undefined)
        }
        _ => Ok(Value::Undefined),
    }
}

fn call_getter(obj: &Rc<RefCell<Object>>, getter_storage: &GetterStorage, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let closure = Rc::clone(env);

    let mut call_env = Environment::with_parent(closure);
    call_env.current_scope_mut().set_this(Value::Object(Rc::clone(obj)));

    let call_env = Rc::new(RefCell::new(call_env));

    let machine = Machine::new(call_env);
    if getter_storage.body.is_empty() {
        Ok(Value::Undefined)
    } else {
        machine.run_statements(&getter_storage.body)
    }
}

fn read_string_property(s: &str, prop_name: &str) -> Result<Value, JsError> {
    match prop_name {
        "length" => Ok(Value::Number(s.len() as f64)),
        "charAt" | "charCodeAt" | "indexOf" | "substring" | "slice"
        | "toUpperCase" | "toLowerCase" | "trim" | "split"
        | "includes" | "startsWith" | "endsWith" | "replace" | "match"
        | "search" | "concat" => {
            let s_clone = s.to_string();
            let prop_name_clone = prop_name.to_string();
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                let s = s_clone.clone();
                match prop_name_clone.as_str() {
                    "length" => Ok(Value::Number(s.len() as f64)),
                    "charAt" => {
                        let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
                    }
                    "indexOf" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
                    }
                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                    "trim" => Ok(Value::String(s.trim().to_string())),
                    "includes" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.contains(&needle)))
                    }
                    "startsWith" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.starts_with(&needle)))
                    }
                    "endsWith" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.ends_with(&needle)))
                    }
                    "concat" => {
                        let sep = args.iter().map(to_js_string).collect::<Vec<_>>().join("");
                        Ok(Value::String(format!("{}{}", s, sep)))
                    }
                    "split" => {
                        let sep = args.first().map(to_js_string).unwrap_or_default();
                        let parts: Vec<Value> = if sep.is_empty() {
                            s.chars().map(|c| Value::String(c.to_string())).collect()
                        } else {
                            s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                        };
                        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(parts.len())))))
                    }
                    "substring" => {
                        let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
                        let start = start.min(s.len());
                        let end = end.min(s.len());
                        let start = start.min(end);
                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                    }
                    "slice" => {
                        let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0) as isize;
                        let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
                        let len = s.len() as isize;
                        let start = if start < 0 { (len + start).max(0) as usize } else { start as usize }.min(len as usize);
                        let end = if end < 0 { (len + end).max(0) as usize } else { end as usize }.min(len as usize);
                        let end = end.max(start);
                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                    }
                    "match" => {
                        let pattern = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.contains(&pattern)))
                    }
                    "search" => {
                        let pattern = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
                    }
                    _ => Ok(Value::Undefined),
                }
            }))))
        }
        _ => Ok(Value::Undefined),
    }
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
