//! Generator function support — function* and yield.
//!
//! Generators are implemented as objects with a `GeneratorState` that tracks
//! the function body, environment, and current position.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement};
use crate::env::Environment;
use crate::value::{Object, ObjectKind, Value};
use crate::JsError;

/// Generator state
#[derive(Debug, Clone, PartialEq)]
pub enum GeneratorState {
    Suspended,
    Running,
    Completed,
}

/// A generator object created by calling a generator function.
#[derive(Debug, Clone)]
pub struct GeneratorObject {
    pub body: Vec<Statement>,
    pub params: Vec<crate::ast::Param>,
    pub closure: Rc<RefCell<Environment>>,
    pub strict: bool,
    pub state: GeneratorState,
    pub yield_index: usize,
    pub yielded_value: Value,
    pub next_value: Value,
    pub is_async: bool,
    pub prototype: Option<Rc<RefCell<Object>>>,
}

impl GeneratorObject {
    pub fn new(
        body: Vec<Statement>,
        params: Vec<crate::ast::Param>,
        closure: Rc<RefCell<Environment>>,
        strict: bool,
    ) -> Self {
        GeneratorObject {
            body,
            params,
            closure,
            strict,
            state: GeneratorState::Suspended,
            yield_index: 0,
            yielded_value: Value::Undefined,
            next_value: Value::Undefined,
            is_async: false,
            prototype: None,
        }
    }

    /// Advance the generator by one step.
    pub fn next(&mut self, value: Value) -> Result<IteratorResult, JsError> {
        if self.state == GeneratorState::Completed {
            return Ok(IteratorResult { value: Value::Undefined, done: true });
        }
        self.state = GeneratorState::Running;
        self.next_value = value;

        // Store the resume value so yield expressions can find it
        crate::interpreter::set_generator_resume_value(self.next_value.clone());

        // Create a fresh call environment
        let call_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&self.closure))));

        // Set up `this` binding
        let global_this = self.closure.borrow().get("globalThis").unwrap_or(Value::Undefined);
        call_env.borrow_mut().current_scope().borrow_mut().set_this(global_this);

        let prev_strict = crate::interpreter::is_strict_mode();
        crate::interpreter::set_strict_mode(self.strict);

        let mut yield_count = 0;
        let mut last_val = Value::Undefined;
        let mut done = false;

        for stmt in &self.body {
            if yield_count < self.yield_index {
                // Skip past yield points we've already passed
                let ys = count_yields_in_stmt(stmt);
                yield_count += ys;
                if yield_count <= self.yield_index {
                    continue;
                }
            }

            match crate::eval::eval_statement(stmt, &call_env, false, false) {
                Ok(val) => {
                    last_val = val;
                    // Check for yield
                    if let Some(yield_val) = crate::interpreter::take_generator_yield() {
                        self.yielded_value = yield_val;
                        self.yield_index = yield_count + 1;
                        self.state = GeneratorState::Suspended;
                        crate::interpreter::set_strict_mode(prev_strict);
                        return Ok(IteratorResult {
                            value: self.yielded_value.clone(),
                            done: false,
                        });
                    }
                    // Check for return
                    if let Some(return_val) = crate::interpreter::take_generator_return() {
                        last_val = return_val;
                        done = true;
                        break;
                    }
                }
                Err(e) => {
                    self.state = GeneratorState::Completed;
                    crate::interpreter::set_strict_mode(prev_strict);
                    return Err(e);
                }
            }
            yield_count += count_yields_in_stmt(stmt);
        }

        self.state = GeneratorState::Completed;
        crate::interpreter::set_strict_mode(prev_strict);
        Ok(IteratorResult { value: last_val, done: true })
    }
}

fn count_yields_in_stmt(stmt: &Statement) -> usize {
    match stmt {
        Statement::Expression(expr) => count_yields_in_expr(expr),
        Statement::Return(Some(expr)) => count_yields_in_expr(expr),
        _ => 0,
    }
}

fn count_yields_in_expr(expr: &Expression) -> usize {
    match expr {
        Expression::Yield(_) => 1,
        Expression::YieldDelegate(_) => 1,
        _ => 0,
    }
}

/// Result of a generator step
#[derive(Debug, Clone)]
pub struct IteratorResult {
    pub value: Value,
    pub done: bool,
}

impl IteratorResult {
    pub fn to_object(&self) -> Value {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("value", self.value.clone());
        obj.set("done", Value::Boolean(self.done));
        Value::Object(Rc::new(RefCell::new(obj)))
    }
}
