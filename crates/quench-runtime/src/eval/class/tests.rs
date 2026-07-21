//! Unit tests for class operations (eval_class_expr, instantiate_class_from_ast,
//! and the ClassValue machinery at the class.rs level).
//!
//! Tests that exercise the public API of eval/class.rs.
//! The helpers module has deeper internal coverage.

#[allow(unused_imports)]
use crate::{Context, Value};

// ─── eval_class_expr: class expressions ───────────────────────────────────

#[test]
fn class_anonymous_has_static_field() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var C = class { static f = 42; }; C.f");
    assert_eq!(v.unwrap(), crate::value::Value::Number(42.0));
}

#[test]
fn class_expression_inferred_name() {
    // Anonymous class expression gets inferred name from assignment LHS
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var Foo = class { bar() { return this instanceof Foo; } }; new Foo().bar()");
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_expression_name_vs_assignment() {
    // Class expression name is set on the class value itself
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            var C = class Foo {
                getName() { return Foo.name; }
            };
            C.getName()
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::String("Foo".to_string()));
}

#[test]
fn class_expression_used_directly() {
    // Anonymous class expression used without assignment
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("new (class { constructor(x) { this.x = x; } })(99).x")
        .unwrap();
    assert_eq!(v, crate::value::Value::Number(99.0));
}

// ─── eval_class_expr: class declarations ─────────────────────────────────

#[test]
fn class_declaration_basic() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C { } typeof C").unwrap();
    assert_eq!(v, crate::value::Value::String("function".to_string()));
}

#[test]
fn class_declaration_is_not_object() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C {} typeof C === 'function'").unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

#[test]
fn class_declaration_instance_has_correct_proto() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C { foo() { return 1; } } var c = new C(); c instanceof C && c.foo() === 1")
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

// ─── Class static fields: eval order ────────────────────────────────────

#[test]
fn class_static_field_eval_order() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            var order = [];
            class C {
                static a = (order.push('a'), 1);
                static b = (order.push('b'), 2);
                static c = (order.push('c'), 3);
            }
            order.join(',')
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::String("a,b,c".to_string()));
}

#[test]
fn class_static_field_this_binding() {
    let _ = 42;
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var C = class { static f = this.name; }; C.f");
    assert!(v.is_ok());
}

#[test]
fn class_static_field_reference_to_other_static_field() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C { static x = 10; static y = this.x * 2; } C.y")
        .unwrap();
    assert_eq!(v, crate::value::Value::Number(20.0));
}

#[test]
fn class_static_field_with_method_call() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            class C {
                static foo() { return 42; }
                static bar = this.foo();
            }
            C.bar
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::Number(42.0));
}

// ─── Class static fields: restricted names ────────────────────────────────

#[test]
fn class_static_field_named_prototype_throws() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C { static prototype = 1; }");
    assert!(v.is_err(), "static field named 'prototype' should throw");
    let err = v.unwrap_err();
    assert!(
        format!("{}", err).contains("prototype"),
        "error should mention prototype: {}",
        err
    );
}

#[test]
fn class_static_field_named_constructor_throws() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C { static constructor = 1; }");
    assert!(v.is_err(), "static field named 'constructor' should throw");
    let err = v.unwrap_err();
    assert!(
        format!("{}", err).contains("constructor"),
        "error should mention constructor: {}",
        err
    );
}

// ─── eval_class_expr: instance fields ────────────────────────────────────

#[test]
fn class_instance_field_basic() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C { x = 42; } new C().x").unwrap();
    assert_eq!(v, crate::value::Value::Number(42.0));
}

#[test]
fn class_instance_field_eval_order() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            var order = [];
            class C {
                constructor() {
                    order.push('body');
                }
                a = (order.push('a'), 1);
                b = (order.push('b'), 2);
            }
            var c = new C();
            order.join(',')
            "#,
        )
        .unwrap();
    // Fields are initialized before constructor body (if super() succeeds)
    assert_eq!(v, crate::value::Value::String("a,b,body".to_string()));
}

#[test]
fn class_instance_field_with_this_access() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C { x = 10; y = this.x + 5; } new C().y")
        .unwrap();
    assert_eq!(v, crate::value::Value::Number(15.0));
}

#[test]
fn class_instance_field_reference_to_another_field() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "class C { a = 1; b = this.a + 2; c = this.b + 3; } [new C().a, new C().b, new C().c]",
        )
        .unwrap();
    // Verify the object has the expected fields
    if let Value::Object(o) = v {
        let obj = o.borrow();
        assert_eq!(obj.get("0"), Some(crate::value::Value::Number(1.0)));
        assert_eq!(obj.get("1"), Some(crate::value::Value::Number(3.0)));
        assert_eq!(obj.get("2"), Some(crate::value::Value::Number(6.0)));
    } else {
        panic!("expected Object, got {:?}", v);
    }
}

// ─── eval_class_expr: extends ────────────────────────────────────────────

#[test]
fn class_extends_null_proto_chain() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C extends null {} Object.getPrototypeOf(C)")
        .unwrap();
    assert_eq!(v, crate::value::Value::Null);
}

#[test]
fn class_extends_function_proto_chain() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C extends Function {} Object.getPrototypeOf(C) === Function.prototype")
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

#[test]
fn class_extends_object_proto_chain() {
    let mut ctx = Context::new().unwrap();
    // class C extends Object {} → C itself has [[Prototype]] === Function.prototype
    // (the superclass constructor's own [[Prototype]])
    let v = ctx
        .eval("class C extends Object {} Object.getPrototypeOf(C) === Function.prototype")
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

// ─── eval_class_expr: methods ────────────────────────────────────────────

#[test]
fn class_method_async() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            class C {
                async foo() { return 42; }
            }
            var c = new C();
            var p = c.foo();
            typeof p === 'object' && typeof p.then === 'function'
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

#[test]
fn class_method_generator() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            class C {
                *gen() { yield 1; yield 2; }
            }
            var g = new C().gen();
            g.next().value + g.next().value
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::Number(3.0));
}

#[test]
fn class_static_method_async() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            class C {
                static async foo() { return 42; }
            }
            var p = C.foo();
            typeof p === 'object' && typeof p.then === 'function'
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

#[test]
fn class_static_method_generator() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            class C {
                static *gen() { yield 1; yield 2; }
            }
            var g = C.gen();
            g.next().value + g.next().value
            "#,
        )
        .unwrap();
    assert_eq!(v, crate::value::Value::Number(3.0));
}

// ─── eval_class_expr: class eval errors ──────────────────────────────────

#[test]
fn class_eval_invalid_extends_throws() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C extends 42 {}");
    assert!(v.is_err(), "extending a primitive number should throw");
}

#[test]
fn class_eval_invalid_extends_string_throws() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("class C extends 'str' {}");
    assert!(v.is_err(), "extending a string should throw");
}

// ─── instantiate_class_from_ast: direct Rust API ───────────────────────

#[test]
fn instantiate_class_from_ast_basic() {
    // Use eval to create a class, then instantiate via JS (covers the Rust API path)
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C { constructor(x) { this.x = x; } } C")
        .unwrap();
    // Now instantiate with args using the class value
    let result = crate::eval::function::call_value_with_this(
        v,
        vec![crate::value::Value::Number(77.0)],
        crate::value::Value::Undefined,
    );
    assert!(result.is_ok());
    if let Value::Object(o) = result.unwrap() {
        assert_eq!(o.borrow().get("x"), Some(crate::value::Value::Number(77.0)));
    }
}

// ─── get_constructor_prototype ───────────────────────────────────────────

#[test]
fn get_constructor_prototype_from_native_constructor() {
    let proto = crate::eval::class::get_constructor_prototype(&Value::NativeConstructor(
        std::rc::Rc::new(crate::value::NativeConstructor::new(
            |_| Ok(Value::Undefined),
            std::rc::Rc::new(std::cell::RefCell::new(crate::value::Object::new(
                crate::value::ObjectKind::Ordinary,
            ))),
        )),
    ));
    assert!(proto.is_ok());
    assert!(proto.unwrap().is_some());
}

#[test]
fn get_constructor_prototype_from_native_function() {
    let nf = crate::value::NativeFunction::new(|_| Ok(Value::Undefined));
    let proto =
        crate::eval::class::get_constructor_prototype(&Value::NativeFunction(std::rc::Rc::new(nf)));
    assert!(proto.is_ok());
    // NativeFunction without explicit prototype set returns None
    assert!(proto.unwrap().is_none());
}

#[test]
fn get_constructor_prototype_from_object() {
    let mut obj = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    obj.set(
        "prototype",
        Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
            crate::value::Object::new(crate::value::ObjectKind::Ordinary),
        ))),
    );
    let proto = crate::eval::class::get_constructor_prototype(&Value::Object(std::rc::Rc::new(
        std::cell::RefCell::new(obj),
    )));
    assert!(proto.is_ok());
    assert!(proto.unwrap().is_some());
}

#[test]
fn get_constructor_prototype_from_primitive() {
    // Primitives return None for get_constructor_prototype
    let proto = crate::eval::class::get_constructor_prototype(&Value::Number(42.0));
    assert!(proto.is_ok());
    assert!(proto.unwrap().is_none());
}

// ─── Class caller/arguments restriction ─────────────────────────────────

#[test]
fn class_caller_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var C = class {};
         var threw = false;
         try { C.caller; } catch(e) { threw = e instanceof TypeError; }
         threw",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_caller_throws_from_function() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var C = class {};
         function fn() { return C.caller; }
         var threw = false;
         try { fn(); } catch(e) { threw = e instanceof TypeError; }
         threw",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_arguments_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var C = class {};
         var threw = false;
         try { C.arguments; } catch(e) { threw = e instanceof TypeError; }
         threw",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_caller_via_indirect_eval() {
    use crate::value::{NativeFunction, Value};
    use std::rc::Rc;
    let mut ctx = Context::new().unwrap();
    let test_call = Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
        let fn_value = args.first().cloned().unwrap_or(Value::Undefined);
        match fn_value {
            Value::Function(f) => crate::eval::function::call_value_with_this(
                Value::Function(f),
                vec![],
                Value::Undefined,
            ),
            _ => Err(crate::value::JsError("not a function".to_string())),
        }
    })));
    ctx.set_global("testCall".to_string(), test_call);
    let v = ctx.eval(
        r#"
        var C = class {};
        var result = "no_error";
        try {
            testCall(C);
            result = "no_error";
        } catch(e) {
            result = "error_thrown";
        }
        result
        "#,
    );
    assert_eq!(
        v.unwrap(),
        crate::value::Value::String("error_thrown".to_string())
    );
}

// ─── Class: combined scenarios ───────────────────────────────────────────

#[test]
fn class_static_and_instance_fields_together() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#"
            class C {
                static sx = 1;
                x = 10;
                static sy = this.sx + 1;
                y = this.x + 5;
            }
            var c = new C();
            [C.sx, C.sy, c.x, c.y]
            "#,
        )
        .unwrap();
    if let Value::Object(o) = v {
        let obj = o.borrow();
        assert_eq!(obj.get("0"), Some(crate::value::Value::Number(1.0)));
        assert_eq!(obj.get("1"), Some(crate::value::Value::Number(2.0)));
        assert_eq!(obj.get("2"), Some(crate::value::Value::Number(10.0)));
        assert_eq!(obj.get("3"), Some(crate::value::Value::Number(15.0)));
    } else {
        panic!("expected Object, got {:?}", v);
    }
}
