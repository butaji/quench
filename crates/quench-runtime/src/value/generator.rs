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
    pub body: std::rc::Rc<Vec<Statement>>,
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
        body: std::rc::Rc<Vec<Statement>>,
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
        for stmt in self.body.iter() {
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

/// Create a NativeFunction that calls GeneratorObject::next().
pub fn generator_next_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let result = gen.borrow_mut().next(arg)?;
            Ok(result.to_object())
        },
    )))
}

/// Create a NativeFunction that calls GeneratorObject::return().
pub fn generator_return_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let mut g = gen.borrow_mut();
            g.state = GeneratorState::Completed;
            Ok(IteratorResult { value: arg, done: true }.to_object())
        },
    )))
}

/// Create a NativeFunction that calls GeneratorObject::throw().
pub fn generator_throw_fn(gen: Rc<RefCell<GeneratorObject>>) -> Value {
    Value::NativeFunction(std::rc::Rc::new(crate::value::NativeFunction::new(
        move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let mut g = gen.borrow_mut();
            g.state = GeneratorState::Completed;
            Err(crate::value::JsError(format!("Generator threw: {:?}", arg)))
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_generator_state_eq() {
        assert_eq!(GeneratorState::Suspended, GeneratorState::Suspended);
        assert_eq!(GeneratorState::Running, GeneratorState::Running);
        assert_eq!(GeneratorState::Completed, GeneratorState::Completed);
    }

    #[test]
    fn test_generator_state_neq() {
        assert_ne!(GeneratorState::Suspended, GeneratorState::Running);
        assert_ne!(GeneratorState::Suspended, GeneratorState::Completed);
        assert_ne!(GeneratorState::Running, GeneratorState::Completed);
    }

    #[test]
    fn test_generator_new_defaults() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![Statement::Expression(Box::new(Expression::Number(1.0)))]);
        let gen = GeneratorObject::new(body, vec![], env, true);
        assert_eq!(gen.state, GeneratorState::Suspended);
        assert_eq!(gen.yield_index, 0);
        assert_eq!(gen.yielded_value, Value::Undefined);
        assert_eq!(gen.next_value, Value::Undefined);
        assert!(gen.strict);
        assert!(!gen.is_async);
        assert!(gen.prototype.is_none());
    }

    #[test]
    fn test_generator_next_empty_body() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![]);
        let mut gen = GeneratorObject::new(body, vec![], env, false);
        let result = gen.next(Value::Undefined).unwrap();
        assert!(result.done);
        assert_eq!(result.value, Value::Undefined);
        assert_eq!(gen.state, GeneratorState::Completed);
    }

    #[test]
    fn test_generator_next_already_completed() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![]);
        let mut gen = GeneratorObject::new(body, vec![], env, false);
        gen.state = GeneratorState::Completed;
        let result = gen.next(Value::Number(99.0)).unwrap();
        assert!(result.done);
        assert_eq!(result.value, Value::Undefined);
        // Still completed
        assert_eq!(gen.state, GeneratorState::Completed);
    }

    #[test]
    fn test_iterator_result_undone() {
        let ir = IteratorResult { value: Value::Number(42.0), done: false };
        let obj_val = ir.to_object();
        let obj = match obj_val {
            Value::Object(ref o) => o,
            _ => panic!("Expected Object"),
        };
        assert_eq!(obj.borrow().get("value"), Some(Value::Number(42.0)));
        assert_eq!(obj.borrow().get("done"), Some(Value::Boolean(false)));
    }

    #[test]
    fn test_iterator_result_done() {
        let ir = IteratorResult { value: Value::String("fin".into()), done: true };
        let obj_val = ir.to_object();
        let obj = match obj_val {
            Value::Object(ref o) => o,
            _ => panic!("Expected Object"),
        };
        assert_eq!(obj.borrow().get("value"), Some(Value::String("fin".into())));
        assert_eq!(obj.borrow().get("done"), Some(Value::Boolean(true)));
    }

    #[test]
    fn test_generator_next_fn_returns_native_fn() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let gen = Rc::new(RefCell::new(GeneratorObject::new(
            Rc::new(vec![]), vec![], env, false,
        )));
        assert!(matches!(generator_next_fn(gen), Value::NativeFunction(_)));
    }

    #[test]
    fn test_generator_return_fn_returns_native_fn() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let gen = Rc::new(RefCell::new(GeneratorObject::new(
            Rc::new(vec![]), vec![], env, false,
        )));
        assert!(matches!(generator_return_fn(gen), Value::NativeFunction(_)));
    }

    #[test]
    fn test_generator_throw_fn_returns_native_fn() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let gen = Rc::new(RefCell::new(GeneratorObject::new(
            Rc::new(vec![]), vec![], env, false,
        )));
        assert!(matches!(generator_throw_fn(gen), Value::NativeFunction(_)));
    }

    #[test]
    fn test_count_yields_in_expr() {
        assert_eq!(count_yields_in_expr(&Expression::Yield(None)), 1);
        assert_eq!(
            count_yields_in_expr(&Expression::Yield(Some(Box::new(Expression::Number(1.0))))),
            1,
        );
        assert_eq!(
            count_yields_in_expr(&Expression::YieldDelegate(Box::new(Expression::Identifier("x".into())))),
            1,
        );
        assert_eq!(count_yields_in_expr(&Expression::Number(42.0)), 0);
        assert_eq!(count_yields_in_expr(&Expression::Boolean(true)), 0);
    }

    #[test]
    fn test_count_yields_in_stmt() {
        assert_eq!(
            count_yields_in_stmt(&Statement::Expression(Box::new(Expression::Yield(None)))),
            1,
        );
        assert_eq!(
            count_yields_in_stmt(&Statement::Return(Some(Box::new(Expression::Yield(None))))),
            1,
        );
        assert_eq!(
            count_yields_in_stmt(&Statement::VarDeclaration {
                kind: crate::ast::VarKind::Let,
                name: "x".into(),
                init: None,
            }),
            0,
        );
    }

    #[test]
    fn test_generator_clone() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let body = Rc::new(vec![]);
        let gen = GeneratorObject::new(body, vec![], env, true);
        let gen_clone = gen.clone();
        assert_eq!(gen.state, gen_clone.state);
        assert_eq!(gen.yield_index, gen_clone.yield_index);
        assert_eq!(gen.strict, gen_clone.strict);
    }

    #[test]
    fn test_generator_debug_output() {
        let gen_str = format!("{:?}", GeneratorState::Suspended);
        assert!(!gen_str.is_empty());
    }

    /// Test that a generator with a simple yield body returns properly.
    /// This tests via JS eval to verify the full stack works.
    #[test]
    fn test_generator_via_eval_create() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval("function* g() { yield 1; } typeof g").unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn test_generator_via_eval_call_returns_generator_object() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "function* g() { yield 1; } let gen = g(); typeof gen"
        ).unwrap();
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_generator_via_eval_next_method_exists() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "function* g() { yield 1; } let gen = g(); typeof gen.next"
        ).unwrap();
        assert_eq!(result, Value::String("function".into()));
    }

    #[test]
    fn test_generator_via_eval_next_returns_object() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "function* g() { yield 1; } let gen = g(); typeof gen.next()"
        ).unwrap();
        assert_eq!(result, Value::String("object".into()));
    }

    #[test]
    fn test_generator_via_eval_next_value() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "function* g() { yield 1; } let gen = g(); gen.next().value"
        ).unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_generator_via_eval_next_done() {
        let mut ctx = crate::Context::new().unwrap();
        // A generator with one yield: first next() returns {value: 1, done: false}
        let done = ctx.eval(
            "function* g() { yield 1; } let gen = g(); gen.next().done"
        ).unwrap();
        assert_eq!(done, Value::Boolean(false));

        // Second next() should return {value: undefined, done: true}
        let done2 = ctx.eval(
            "function* g() { yield 1; } let gen = g(); gen.next(); gen.next().done"
        ).unwrap();
        assert_eq!(done2, Value::Boolean(true));
    }

    #[test]
    fn test_generator_via_eval_multiple_yields() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "function* g() { yield 1; yield 2; yield 3; } \
             let gen = g(); \
             let a = gen.next().value; \
             let b = gen.next().value; \
             let c = gen.next().value; \
             [a, b, c]"
        ).unwrap();
        // Check array result
        match result {
            Value::Object(ref obj) => {
                let arr = obj.borrow();
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::Number(2.0)));
                assert_eq!(arr.elements.get(2), Some(&Value::Number(3.0)));
            }
            _ => panic!("Expected array object"),
        }
    }

    #[test]
    fn test_generator_return_method() {
        let mut ctx = crate::Context::new().unwrap();
        let done = ctx.eval(
            "function* g() { yield 1; yield 2; } \
             let gen = g(); \
             gen.next(); \
             gen.return(99).value"
        ).unwrap();
        assert_eq!(done, Value::Number(99.0));
    }

    #[test]
    fn test_generator_throw_method() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "function* g() { yield 1; } \
             let gen = g(); \
             try { gen.throw(new Error('test')); 'no_error' } catch(e) { 'error' }"
        ).unwrap();
        assert_eq!(result, Value::String("error".into()));
    }
}
