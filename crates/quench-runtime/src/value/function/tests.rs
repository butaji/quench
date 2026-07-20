//! Unit tests for value/function.rs — ValueFunction, NativeFunction, NativeConstructor.

#[allow(unused_imports)]
use crate::ast::{ArrowBody, Expression, Param};
#[allow(unused_imports)]
use crate::env::Environment;
#[allow(unused_imports)]
use crate::value::function::{
    expected_argument_count, ConstructorAccessor, NativeConstructor, NativeFunction, ValueFunction,
};
#[allow(unused_imports)]
use crate::value::object::helpers::PropertyFlags;
#[allow(unused_imports)]
use crate::value::{JsError, Value};
#[allow(unused_imports)]
use std::cell::RefCell;
#[allow(unused_imports)]
use std::rc::Rc;

// ── expected_argument_count ────────────────────────────────────────────────────

#[test]
fn expected_arg_count_empty() {
    assert_eq!(expected_argument_count(&[]), 0.0);
}

#[test]
fn expected_arg_count_no_defaults() {
    let params = vec![Param::new("a"), Param::new("b"), Param::new("c")];
    assert_eq!(expected_argument_count(&params), 3.0);
}

#[test]
fn expected_arg_count_stops_at_default() {
    let mut p1 = Param::new("a");
    p1.default = Some(Box::new(Expression::Number(1.0)));
    let p2 = Param::new("b");
    let p3 = Param::new("c");

    let params = vec![p1, p2, p3];
    assert_eq!(expected_argument_count(&params), 0.0);
}

#[test]
fn expected_arg_count_stops_at_middle_default() {
    let p1 = Param::new("a");
    let p2 = Param::new("b");
    let mut p3 = Param::new("c");
    p3.default = Some(Box::new(Expression::Number(1.0)));
    let p4 = Param::new("d");

    let params = vec![p1, p2, p3, p4];
    assert_eq!(expected_argument_count(&params), 2.0);
}

#[test]
fn expected_arg_count_rest_param_ignored() {
    let p1 = Param::new("a");
    let p2 = Param::new("b");
    let mut p3 = Param::new("rest");
    p3.rest = true;

    let params = vec![p1, p2, p3];
    assert_eq!(expected_argument_count(&params), 3.0);
}

// ── ValueFunction construction ────────────────────────────────────────────────

#[test]
fn value_function_new_sets_length_and_name() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let params = vec![Param::new("x"), Param::new("y"), Param::new("z")];
    let func = ValueFunction::new(
        Some("add".into()),
        params,
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    assert_eq!(func.name, Some("add".into()));
    assert_eq!(func.length(), 3);
    assert_eq!(func.get_property("length"), Some(Value::Number(3.0)));
    assert_eq!(func.get_property("name"), Some(Value::String("add".into())));
}

#[test]
fn value_function_new_unnamed_has_empty_name_property() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let params = vec![Param::new("x")];
    let func = ValueFunction::new(None, params, vec![], Rc::clone(&env), false, false);

    assert_eq!(func.name, None);
    assert_eq!(func.get_property("length"), Some(Value::Number(1.0)));
    assert!(func.get_property("name").is_none());
}

#[test]
fn value_function_new_arrow_no_name() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let params = vec![Param::new("x"), Param::new("y")];
    let body = ArrowBody::Expression(Expression::Number(42.0));
    let func = ValueFunction::new_arrow(params, Box::new(body), Rc::clone(&env));

    assert_eq!(func.name, None);
    assert!(func.is_arrow);
    assert_eq!(func.length(), 2);
    assert_eq!(func.get_property("length"), Some(Value::Number(2.0)));
    assert!(func.get_property("name").is_none());
}

#[test]
fn value_function_with_defaults_has_shorter_length() {
    let env = Rc::new(RefCell::new(Environment::new()));

    let mut p1 = Param::new("a");
    p1.default = Some(Box::new(Expression::Number(0.0)));
    let p2 = Param::new("b");

    let params = vec![p1, p2];
    let func = ValueFunction::new(
        Some("f".into()),
        params,
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    assert_eq!(func.length(), 0);
    assert_eq!(func.get_property("length"), Some(Value::Number(0.0)));
}

#[test]
fn value_function_closure_is_stored() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("f".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    assert_eq!(Rc::strong_count(&func.closure), Rc::strong_count(&env));
}

// ── ValueFunction properties ──────────────────────────────────────────────────

#[test]
fn value_function_get_property_returns_copies() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("f".into()),
        vec![Param::new("x")],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    let len1 = func.get_property("length");
    let len2 = func.get_property("length");
    assert_eq!(len1, len2);
}

#[test]
fn value_function_set_property_inserts() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(None, vec![], vec![], Rc::clone(&env), false, false);

    func.set_property("custom", Value::Number(99.0));
    assert_eq!(func.get_property("custom"), Some(Value::Number(99.0)));
}

#[test]
fn value_function_set_property_overwrites() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(None, vec![], vec![], Rc::clone(&env), false, false);

    func.set_property("x", Value::Number(1.0));
    func.set_property("x", Value::Number(2.0));
    assert_eq!(func.get_property("x"), Some(Value::Number(2.0)));
}

#[test]
fn value_function_remove_property_returns_true_when_present() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("f".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    assert!(func.remove_property("length"));
    assert!(func.remove_property("name"));
    assert!(func.get_property("length").is_none());
    assert!(func.get_property("name").is_none());
}

#[test]
fn value_function_remove_property_returns_false_when_absent() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(None, vec![], vec![], Rc::clone(&env), false, false);

    assert!(!func.remove_property("nonexistent"));
}

#[test]
fn value_function_remove_property_idempotent() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("f".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    func.remove_property("length");
    assert!(!func.remove_property("length"));
}

// ── ValueFunction prototype ────────────────────────────────────────────────────

#[test]
fn value_function_has_prototype_false_initially() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(None, vec![], vec![], Rc::clone(&env), false, false);
    assert!(!func.has_prototype());
}

#[test]
fn value_function_get_prototype_creates_and_caches() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(None, vec![], vec![], Rc::clone(&env), false, false);

    let proto1 = func.get_prototype();
    let proto2 = func.get_prototype();
    assert!(Rc::ptr_eq(&proto1, &proto2));
    assert!(func.has_prototype());
}

#[test]
fn value_function_get_prototype_sets_constructor() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(None, vec![], vec![], Rc::clone(&env), false, false);

    let proto = func.get_prototype();
    let ctor = proto.borrow().get("constructor");
    assert!(ctor.is_some());
}

#[test]
fn value_function_identity_ptr_distinct_for_distinct_functions() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let f1 = ValueFunction::new(
        Some("f1".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );
    let f2 = ValueFunction::new(
        Some("f2".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );

    assert_ne!(f1.identity_ptr(), f2.identity_ptr());
}

#[test]
fn value_function_identity_ptr_same_for_clone() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let f1 = ValueFunction::new(
        Some("f".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );
    let f2 = f1.clone();

    assert_eq!(f1.identity_ptr(), f2.identity_ptr());
}

#[test]
fn value_function_clone_shares_properties_map() {
    let env = Rc::new(RefCell::new(Environment::new()));
    let f1 = ValueFunction::new(
        Some("f".into()),
        vec![],
        vec![],
        Rc::clone(&env),
        false,
        false,
    );
    let f2 = f1.clone();

    f1.set_property("x", Value::Number(1.0));
    assert_eq!(f2.get_property("x"), Some(Value::Number(1.0)));
}

// ── NativeFunction construction ────────────────────────────────────────────────

#[test]
fn native_function_new_has_empty_name() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    assert_eq!(nf.name, "");
}

#[test]
fn native_function_new_named_has_name() {
    let nf = NativeFunction::new_named("myFunc", |_args| Ok(Value::Undefined));
    assert_eq!(nf.name, "myFunc");
}

#[test]
fn native_function_new_with_prototype_has_prototype() {
    use crate::value::object::ObjectKind;
    let obj = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nf = NativeFunction::new_with_prototype(|_args| Ok(Value::Undefined), Rc::clone(&obj));
    assert!(nf.prototype.borrow().is_some());
}

#[test]
fn native_function_get_property_empty_initially() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    assert!(nf.get_property("length").is_none());
}

#[test]
fn native_function_set_property_inserts() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    nf.set_property("custom", Value::Number(42.0)).unwrap();
    assert_eq!(nf.get_property("custom"), Some(Value::Number(42.0)));
}

#[test]
fn native_function_set_property_readonly_rejects() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    nf.define_property(
        "locked",
        Value::Number(1.0),
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        },
    );

    let result = nf.set_property("locked", Value::Number(2.0));
    assert!(result.is_err());
    assert_eq!(nf.get_property("locked"), Some(Value::Number(1.0)));
}

#[test]
fn native_function_define_property_sets_flags() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    nf.define_property(
        "flagged",
        Value::Number(7.0),
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );

    let flags = nf.get_property_flags("flagged");
    assert!(flags.is_some());
    let f = flags.unwrap();
    assert!(!f.writable);
    assert!(!f.enumerable);
    assert!(!f.configurable);
}

#[test]
fn native_function_get_property_flags_none_when_undefined() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    assert!(nf.get_property_flags("missing").is_none());
}

#[test]
fn native_function_remove_property_returns_true() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    nf.set_property("temp", Value::Number(1.0)).unwrap();
    nf.define_property("temp2", Value::Number(2.0), PropertyFlags::default_data());

    assert!(nf.remove_property("temp"));
    assert!(nf.get_property("temp").is_none());
    assert!(nf.get_property_flags("temp").is_none());

    assert!(nf.remove_property("temp2"));
}

#[test]
fn native_function_remove_property_returns_false_when_absent() {
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    assert!(!nf.remove_property("missing"));
}

#[test]
fn native_function_call_executes() {
    let nf = NativeFunction::new(|args| {
        let sum: f64 = args
            .iter()
            .filter_map(|v| match v {
                Value::Number(n) => Some(*n),
                _ => None,
            })
            .sum();
        Ok(Value::Number(sum))
    });

    let result = nf.call(
        Value::Undefined,
        vec![Value::Number(1.0), Value::Number(2.0)],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Number(3.0));
}

#[test]
fn native_function_call_returns_error() {
    let nf = NativeFunction::new(|_args| Err(JsError::new("boom")));
    let result = nf.call(Value::Undefined, vec![]);
    assert!(result.is_err());
}

// ── NativeFunction prototype setting ──────────────────────────────────────────

#[test]
fn native_function_set_prototype_sets_constructor_on_object() {
    use crate::value::object::ObjectKind;
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    let obj = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));

    nf.set_property("prototype", Value::Object(Rc::clone(&obj)))
        .unwrap();

    let ctor = obj.borrow().get("constructor");
    assert!(ctor.is_some());
    if let Value::NativeFunction(_) = ctor.unwrap() {
        // ok
    } else {
        panic!("expected NativeFunction constructor");
    }
}

#[test]
fn native_function_set_prototype_stores_in_properties() {
    use crate::value::object::ObjectKind;
    let nf = NativeFunction::new(|_args| Ok(Value::Undefined));
    let obj = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));

    nf.set_property("prototype", Value::Object(Rc::clone(&obj)))
        .unwrap();
    let stored = nf.get_property("prototype");
    assert!(stored.is_some());
    if let Value::Object(stored_rc) = stored.unwrap() {
        assert!(Rc::ptr_eq(&stored_rc, &obj));
    } else {
        panic!("expected Object value");
    }
}

// ── NativeConstructor construction ────────────────────────────────────────────

#[test]
fn native_constructor_new_has_empty_name() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));
    assert_eq!(nc.name(), "");
}

#[test]
fn native_constructor_set_and_get_name() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));

    nc.set_name("MyError");
    assert_eq!(nc.name(), "MyError");
}

#[test]
fn native_constructor_static_methods() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));

    nc.set_static_method("create", Value::Number(42.0));
    assert_eq!(nc.get_static_method("create"), Some(Value::Number(42.0)));
}

#[test]
fn native_constructor_get_static_method_missing() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));
    assert!(nc.get_static_method("missing").is_none());
}

#[test]
fn native_constructor_accessors() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));

    let getter = Value::Number(1.0);
    let setter = Value::Number(2.0);
    nc.define_accessor("foo", Some(getter.clone()), Some(setter.clone()));

    let acc = nc.get_accessor("foo").unwrap();
    assert_eq!(acc.getter, Some(getter));
    assert_eq!(acc.setter, Some(setter));
}

#[test]
fn native_constructor_get_accessor_missing() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));
    assert!(nc.get_accessor("missing").is_none());
}

#[test]
fn native_constructor_call_executes() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(
        |args| {
            let sum: f64 = args
                .iter()
                .filter_map(|v| match v {
                    Value::Number(n) => Some(*n),
                    _ => None,
                })
                .sum();
            Ok(Value::Number(sum))
        },
        Rc::clone(&proto),
    );

    let result = nc.call(
        Value::Undefined,
        vec![Value::Number(5.0), Value::Number(3.0)],
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Number(8.0));
}

#[test]
fn native_constructor_set_property_is_noop() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(|_args| Ok(Value::Undefined), Rc::clone(&proto));

    nc.set_property("anything", Value::Number(1.0));
}

#[test]
fn native_constructor_call_func_directly() {
    use crate::value::object::ObjectKind;
    let proto = Rc::new(RefCell::new(crate::value::object::Object::new(
        ObjectKind::Ordinary,
    )));
    let nc = NativeConstructor::new(
        |args| Ok(Value::Number(args.len() as f64)),
        Rc::clone(&proto),
    );

    let result = nc.call_func(vec![Value::Undefined, Value::Undefined]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Number(2.0));
}

// ── ConstructorAccessor ───────────────────────────────────────────────────────

#[test]
fn constructor_accessor_clone() {
    let getter = Value::Number(1.0);
    let setter = Value::Number(2.0);
    let acc = ConstructorAccessor {
        getter: Some(getter.clone()),
        setter: Some(setter.clone()),
    };
    let cloned = acc.clone();
    assert_eq!(cloned.getter, Some(getter));
    assert_eq!(cloned.setter, Some(setter));
}

#[test]
fn constructor_accessor_partial_none() {
    let acc = ConstructorAccessor {
        getter: None,
        setter: Some(Value::Undefined),
    };
    assert!(acc.getter.is_none());
    assert!(acc.setter.is_some());
}
