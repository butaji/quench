//! Comprehensive unit tests for generator.rs VALUE layer.
//!
//! Tests GeneratorObject struct manipulation and the generator_*_fn functions.
//!
//! Note: async generator functions (async_generator_*) require Promise prototype
//! initialization and are tested via Context::eval in generator.rs.

use quench_runtime::value::generator::{
    generator_next_fn, generator_return_fn, generator_throw_fn, GeneratorObject, GeneratorState,
    IteratorResult,
};
use quench_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

fn make_env() -> Rc<RefCell<quench_runtime::env::Environment>> {
    Rc::new(RefCell::new(quench_runtime::env::Environment::new()))
}

fn make_gen() -> Rc<RefCell<GeneratorObject>> {
    let env = make_env();
    Rc::new(RefCell::new(GeneratorObject::new(
        Rc::new(vec![]),
        vec![],
        env,
        false,
    )))
}

// ─── GeneratorState ─────────────────────────────────────────────────────────

#[test]
fn generator_state_suspended() {
    let gen = make_gen();
    assert_eq!(gen.borrow().state, GeneratorState::Suspended);
}

#[test]
fn generator_state_running() {
    let gen = make_gen();
    let mut g = gen.borrow_mut();
    g.state = GeneratorState::Running;
    assert_eq!(g.state, GeneratorState::Running);
}

#[test]
fn generator_state_completed() {
    let gen = make_gen();
    let mut g = gen.borrow_mut();
    g.state = GeneratorState::Completed;
    assert_eq!(g.state, GeneratorState::Completed);
}

// ─── GeneratorObject::new ────────────────────────────────────────────────────

#[test]
fn generator_object_new_suspended() {
    let env = make_env();
    let gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    assert_eq!(gen.state, GeneratorState::Suspended);
}

#[test]
fn generator_object_new_yield_index_zero() {
    let env = make_env();
    let gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    assert_eq!(gen.yield_index, 0);
}

#[test]
fn generator_object_new_strict_flag() {
    let env = make_env();
    let strict_gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, true);
    assert!(strict_gen.strict);
}

#[test]
fn generator_object_new_non_strict() {
    let env = make_env();
    let non_strict_gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    assert!(!non_strict_gen.strict);
}

#[test]
fn generator_object_new_async_false() {
    let env = make_env();
    let gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    assert!(!gen.is_async);
}

#[test]
fn generator_object_new_yielded_value_undefined() {
    let env = make_env();
    let gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    assert_eq!(gen.yielded_value, Value::Undefined);
}

#[test]
fn generator_object_new_next_value_undefined() {
    let env = make_env();
    let gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    assert_eq!(gen.next_value, Value::Undefined);
}

// ─── GeneratorObject::next ────────────────────────────────────────────────────

#[test]
fn generator_next_empty_body_completes() {
    let env = make_env();
    let mut gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    let result = gen.next(Value::Undefined).unwrap();
    assert!(result.done);
    assert_eq!(result.value, Value::Undefined);
    assert_eq!(gen.state, GeneratorState::Completed);
}

#[test]
fn generator_next_completed_state_returns_done() {
    let gen = make_gen();
    {
        let mut g = gen.borrow_mut();
        g.state = GeneratorState::Completed;
    }
    let result = gen.borrow_mut().next(Value::Number(42.0)).unwrap();
    assert!(result.done);
    assert_eq!(result.value, Value::Undefined);
    assert_eq!(gen.borrow().state, GeneratorState::Completed);
}

#[test]
fn generator_next_with_value_stores_it() {
    let env = make_env();
    let mut gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    gen.next(Value::Number(99.0)).unwrap();
    assert_eq!(gen.next_value, Value::Number(99.0));
}

// ─── IteratorResult ──────────────────────────────────────────────────────────

#[test]
fn iterator_result_to_object_done() {
    let ir = IteratorResult {
        value: Value::Number(42.0),
        done: true,
    };
    let obj = ir.to_object();
    let Value::Object(o) = obj else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::Number(42.0)));
    assert_eq!(o.get("done"), Some(Value::Boolean(true)));
}

#[test]
fn iterator_result_to_object_undone() {
    let ir = IteratorResult {
        value: Value::String("hello".into()),
        done: false,
    };
    let obj = ir.to_object();
    let Value::Object(o) = obj else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::String("hello".into())));
    assert_eq!(o.get("done"), Some(Value::Boolean(false)));
}

#[test]
fn iterator_result_to_object_undefined_value() {
    let ir = IteratorResult {
        value: Value::Undefined,
        done: false,
    };
    let obj = ir.to_object();
    let Value::Object(o) = obj else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::Undefined));
    assert_eq!(o.get("done"), Some(Value::Boolean(false)));
}

#[test]
fn iterator_result_to_object_null_value() {
    let ir = IteratorResult {
        value: Value::Null,
        done: true,
    };
    let obj = ir.to_object();
    let Value::Object(o) = obj else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::Null));
}

// ─── generator_next_fn ───────────────────────────────────────────────────────

#[test]
fn generator_next_fn_returns_native_function() {
    let gen = make_gen();
    let result = generator_next_fn(gen);
    assert!(matches!(result, Value::NativeFunction(_)));
}

#[test]
fn generator_next_fn_called_returns_object() {
    let gen = make_gen();
    let next_fn = generator_next_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = next_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    assert!(matches!(result, Value::Object(_)));
}

#[test]
fn generator_next_fn_with_argument() {
    let gen = make_gen();
    let next_fn = generator_next_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = next_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf
        .call(Value::Undefined, vec![Value::Number(123.0)])
        .unwrap();
    assert!(matches!(result, Value::Object(_)));
}

#[test]
fn generator_next_fn_result_has_done_property() {
    let gen = make_gen();
    let next_fn = generator_next_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = next_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert!(o.get("done").is_some());
}

#[test]
fn generator_next_fn_result_has_value_property() {
    let gen = make_gen();
    let next_fn = generator_next_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = next_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert!(o.get("value").is_some());
}

// ─── generator_return_fn ─────────────────────────────────────────────────────

#[test]
fn generator_return_fn_returns_native_function() {
    let gen = make_gen();
    let result = generator_return_fn(gen);
    assert!(matches!(result, Value::NativeFunction(_)));
}

#[test]
fn generator_return_fn_returns_done_true() {
    let gen = make_gen();
    let return_fn = generator_return_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = return_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("done"), Some(Value::Boolean(true)));
}

#[test]
fn generator_return_fn_returns_passed_value() {
    let gen = make_gen();
    let return_fn = generator_return_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = return_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf
        .call(Value::Undefined, vec![Value::Number(42.0)])
        .unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::Number(42.0)));
}

#[test]
fn generator_return_fn_with_string_value() {
    let gen = make_gen();
    let return_fn = generator_return_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = return_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf
        .call(Value::Undefined, vec![Value::String("done".into())])
        .unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::String("done".into())));
}

#[test]
fn generator_return_fn_sets_state_completed() {
    let gen = make_gen();
    let return_fn = generator_return_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = return_fn else {
        panic!("Expected NativeFunction")
    };
    // Call the function to trigger state mutation
    nf.call(Value::Undefined, vec![Value::Undefined]).unwrap();
    assert_eq!(gen.borrow().state, GeneratorState::Completed);
}

#[test]
fn generator_return_fn_default_undefined() {
    let gen = make_gen();
    let return_fn = generator_return_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = return_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("value"), Some(Value::Undefined));
}

// ─── generator_throw_fn ─────────────────────────────────────────────────────

#[test]
fn generator_throw_fn_returns_native_function() {
    let gen = make_gen();
    let result = generator_throw_fn(gen);
    assert!(matches!(result, Value::NativeFunction(_)));
}

#[test]
fn generator_throw_fn_returns_error() {
    let gen = make_gen();
    let throw_fn = generator_throw_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = throw_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![Value::String("test error".into())]);
    assert!(result.is_err());
}

#[test]
fn generator_throw_fn_sets_state_completed() {
    let gen = make_gen();
    let throw_fn = generator_throw_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = throw_fn else {
        panic!("Expected NativeFunction")
    };
    let _ = nf.call(Value::Undefined, vec![Value::String("error".into())]);
    assert_eq!(gen.borrow().state, GeneratorState::Completed);
}

#[test]
fn generator_throw_fn_error_contains_message() {
    let gen = make_gen();
    let throw_fn = generator_throw_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = throw_fn else {
        panic!("Expected NativeFunction")
    };
    let err = nf
        .call(Value::Undefined, vec![Value::String("custom error".into())])
        .unwrap_err();
    assert!(err.to_string().contains("custom error"));
}

#[test]
fn generator_throw_fn_with_number_error() {
    let gen = make_gen();
    let throw_fn = generator_throw_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = throw_fn else {
        panic!("Expected NativeFunction")
    };
    let err = nf
        .call(Value::Undefined, vec![Value::Number(999.0)])
        .unwrap_err();
    assert!(err.to_string().contains("999"));
}

// ─── async_generator_* functions require Promise prototype init ────────────────
// async_generator_next_fn, async_generator_return_fn, async_generator_throw_fn
// require the Promise prototype to be initialized via Context.
// They are tested via Context::eval in the inline tests in generator.rs.

// ─── Generator cloning ───────────────────────────────────────────────────────

#[test]
fn generator_clone_preserves_state() {
    let env = make_env();
    let gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, true);
    let clone = gen.clone();
    assert_eq!(gen.state, clone.state);
}

#[test]
fn generator_clone_preserves_yield_index() {
    let env = make_env();
    let mut gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, false);
    gen.yield_index = 5;
    let clone = gen.clone();
    assert_eq!(gen.yield_index, clone.yield_index);
}

#[test]
fn generator_clone_preserves_strict() {
    let env = make_env();
    let strict_gen = GeneratorObject::new(Rc::new(vec![]), vec![], env, true);
    let clone = strict_gen.clone();
    assert_eq!(strict_gen.strict, clone.strict);
}

// ─── Value::Generator ────────────────────────────────────────────────────────

#[test]
fn value_generator_roundtrip() {
    let gen = make_gen();
    let val = Value::Generator(Rc::clone(&gen));
    let Value::Generator(g) = val else {
        panic!("Expected Generator")
    };
    assert!(Rc::ptr_eq(&gen, &g));
}

#[test]
fn generator_debug_format() {
    let gen_str = format!("{:?}", GeneratorState::Suspended);
    assert!(gen_str.contains("Suspended"));
    let gen_str = format!("{:?}", GeneratorState::Running);
    assert!(gen_str.contains("Running"));
    let gen_str = format!("{:?}", GeneratorState::Completed);
    assert!(gen_str.contains("Completed"));
}

// ─── Edge cases ───────────────────────────────────────────────────────────────

#[test]
fn generator_next_with_null_value() {
    let gen = make_gen();
    let result = gen.borrow_mut().next(Value::Null).unwrap();
    assert!(result.done);
}

#[test]
fn generator_next_with_boolean_value() {
    let gen = make_gen();
    let result = gen.borrow_mut().next(Value::Boolean(true)).unwrap();
    assert!(result.done);
}

#[test]
fn generator_next_with_string_value() {
    let gen = make_gen();
    let result = gen.borrow_mut().next(Value::String("test".into())).unwrap();
    assert!(result.done);
}

#[test]
fn multiple_next_calls_after_completion() {
    let gen = make_gen();
    // First call completes it
    {
        let mut g = gen.borrow_mut();
        g.next(Value::Undefined).unwrap();
        assert_eq!(g.state, GeneratorState::Completed);
    }
    // Subsequent calls still return done
    let result = gen.borrow_mut().next(Value::Number(1.0)).unwrap();
    assert!(result.done);
}

#[test]
fn generator_return_then_next() {
    let gen = make_gen();
    let return_fn = generator_return_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = return_fn else {
        panic!("Expected NativeFunction")
    };
    nf.call(Value::Undefined, vec![Value::Number(42.0)])
        .unwrap();
    // Now try next - should still be completed
    let next_fn = generator_next_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = next_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("done"), Some(Value::Boolean(true)));
}

#[test]
fn generator_throw_then_next() {
    let gen = make_gen();
    let throw_fn = generator_throw_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = throw_fn else {
        panic!("Expected NativeFunction")
    };
    nf.call(Value::Undefined, vec![Value::String("err".into())])
        .unwrap_err();
    // Next should return done
    let next_fn = generator_next_fn(Rc::clone(&gen));
    let Value::NativeFunction(nf) = next_fn else {
        panic!("Expected NativeFunction")
    };
    let result = nf.call(Value::Undefined, vec![]).unwrap();
    let Value::Object(o) = result else {
        panic!("Expected Object")
    };
    let o = o.borrow();
    assert_eq!(o.get("done"), Some(Value::Boolean(true)));
}
