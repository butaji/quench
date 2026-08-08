use quench_runtime::value::generator::{
    generator_next_fn, generator_return_fn, generator_throw_fn, GeneratorObject, GeneratorState,
};
use quench_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

fn make_gen() -> Rc<RefCell<GeneratorObject>> {
    let env = Rc::new(RefCell::new(quench_runtime::env::Environment::new()));
    Rc::new(RefCell::new(GeneratorObject::new(
        Rc::new(vec![]),
        vec![],
        env,
        false,
    )))
}

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

#[test]
fn generator_next_with_null_value() {
    let gen = make_gen();
    assert!(gen.borrow_mut().next(Value::Null).unwrap().done);
}

#[test]
fn generator_next_with_boolean_value() {
    let gen = make_gen();
    assert!(gen.borrow_mut().next(Value::Boolean(true)).unwrap().done);
}

#[test]
fn generator_next_with_string_value() {
    let gen = make_gen();
    assert!(
        gen.borrow_mut()
            .next(Value::String("test".into()))
            .unwrap()
            .done
    );
}

#[test]
fn multiple_next_calls_after_completion() {
    let gen = make_gen();
    gen.borrow_mut().next(Value::Undefined).unwrap();
    assert_eq!(gen.borrow().state, GeneratorState::Completed);
    assert!(gen.borrow_mut().next(Value::Number(1.0)).unwrap().done);
}

#[test]
fn generator_return_then_next() {
    let gen = make_gen();
    let Value::NativeFunction(nf) = generator_return_fn(Rc::clone(&gen)) else {
        panic!("Expected NativeFunction")
    };
    nf.call(Value::Undefined, vec![Value::Number(42.0)])
        .unwrap();
    let Value::NativeFunction(nf) = generator_next_fn(Rc::clone(&gen)) else {
        panic!("Expected NativeFunction")
    };
    let Value::Object(o) = nf.call(Value::Undefined, vec![]).unwrap() else {
        panic!("Expected Object")
    };
    assert_eq!(o.borrow().get("done"), Some(Value::Boolean(true)));
}

#[test]
fn generator_throw_then_next() {
    let gen = make_gen();
    let Value::NativeFunction(nf) = generator_throw_fn(Rc::clone(&gen)) else {
        panic!("Expected NativeFunction")
    };
    nf.call(Value::Undefined, vec![Value::String("err".into())])
        .unwrap_err();
    let Value::NativeFunction(nf) = generator_next_fn(Rc::clone(&gen)) else {
        panic!("Expected NativeFunction")
    };
    let Value::Object(o) = nf.call(Value::Undefined, vec![]).unwrap() else {
        panic!("Expected Object")
    };
    assert_eq!(o.borrow().get("done"), Some(Value::Boolean(true)));
}
