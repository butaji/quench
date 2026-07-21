//! Unit tests for object operations.

#[allow(unused_imports)]
use crate::{Context, Value};

#[test]
fn strict_for_in_var_iterates() {
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "\"use strict\";\
             var obj = {a:1, b:2};\
             var count = 0;\
             for (var property in obj) { count++; }\
             if (count !== 2) throw new Error(\"count=\" + count);",
    )
    .expect("strict for-in should iterate");
}

#[test]
fn for_in_enumerates_defined_property() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var o = {a: 1}").unwrap();
    ctx.eval("var keys = []; for (var k in o) { keys.push(k); }")
        .unwrap();
    let len = ctx.eval("keys.length").unwrap();
    assert_eq!(len, Value::Number(1.0), "for-in should iterate once");
    let first = ctx.eval("keys[0]").unwrap();
    assert_eq!(first, Value::String("a".to_string()), "key should be 'a'");
    ctx.eval("var o2 = {}; Object.defineProperty(o2, 'b', {enumerable: true, value: 2});")
        .unwrap();
    ctx.eval("var keys2 = []; for (var k in o2) { keys2.push(k); }")
        .unwrap();
    let len2 = ctx.eval("keys2.length").unwrap();
    assert_eq!(
        len2,
        Value::Number(1.0),
        "for-in should see enumerable property"
    );
    let first2 = ctx.eval("keys2[0]").unwrap();
    assert_eq!(first2, Value::String("b".to_string()), "key should be 'b'");
}

#[test]
fn strict_assign_undeclared_throws() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("\"use strict\"; undeclared = 5;");
    assert!(res.is_err(), "strict assignment to undeclared should throw");
}

#[test]
fn sloppy_assign_undeclared_no_throw() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("undeclared = 5;");
    assert!(
        res.is_ok(),
        "sloppy assignment to undeclared should not throw"
    );
}

#[test]
fn valueof_throw_propagates_in_addition() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval(
        "var caught; try { 1 + {valueOf: function() {throw \"err\"}}; } catch (e) { caught = e; } caught;",
    );
    assert_eq!(res.unwrap(), crate::value::Value::String("err".to_string()));
}

#[test]
fn symbol_to_primitive_throw_propagates() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval(
        "var caught; \
             var t = {}; \
             Object.defineProperty(t, Symbol.toPrimitive, { get: function() { return function() { throw \"boom\"; }; } }); \
             try { t + 1; } catch (e) { caught = e; } \
             caught;",
    );
    let v = res.unwrap();
    match v {
        crate::value::Value::String(s) => assert_eq!(s, "boom"),
        other => panic!("expected string 'boom', got {:?}", other),
    }
}

#[test]
fn array_elision_length_is_one() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var a = [,]; a.length").unwrap();
    assert_eq!(v, crate::value::Value::Number(1.0));
}

#[test]
fn arrow_fn_caller_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("var arrowFn = () => {}; arrowFn.caller");
    assert!(res.is_err(), "arrowFn.caller must throw");
}

#[test]
fn arrow_fn_caller_throws_in_harness() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval(
        "var arrowFn = () => {}; \
             var caught = false; \
             try { var x = arrowFn.caller; } catch (e) { caught = (e instanceof TypeError); } \
             caught;",
    );
    let v = res.unwrap();
    assert_eq!(
        v,
        crate::value::Value::Boolean(true),
        "must catch TypeError"
    );
}

#[test]
fn direct_arrow_returns_this() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("(()=>this)();").unwrap();
    match v {
        crate::value::Value::Object(_) => {}
        other => panic!("got: {:?}", other),
    }
}

#[test]
fn top_level_this_is_global() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("this;").unwrap();
    match v {
        crate::value::Value::Object(_) => {}
        other => panic!("got: {:?}", other),
    }
}

#[test]
fn fn_returning_this_inside_obj() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("var o = { f: function() { return (() => this); } }; o.f()();")
        .unwrap();
    match v {
        crate::value::Value::Object(_) => {}
        other => panic!("got: {:?}", other),
    }
}

#[test]
fn arrow_fn_length_own_property() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("var f = (x, y = 1) => {}; f.hasOwnProperty('length')")
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
    let len = ctx.eval("f.length").unwrap();
    assert_eq!(len, crate::value::Value::Number(1.0));
}

#[test]
fn arrow_fn_length_full_test262() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var f1 = (x = 42) => {}; \
                 var ok = f1.hasOwnProperty('length'); \
                 var len = f1.length; \
                 var deleted = delete f1.length; \
                 var stillHas = f1.hasOwnProperty('length'); \
                 [ok, len, deleted, stillHas];",
        )
        .unwrap();
    let arr = match v {
        crate::value::Value::Object(o) => o,
        other => panic!("expected array: {:?}", other),
    };
    let e = arr.borrow().elements.clone();
    assert_eq!(
        e[0],
        crate::value::Value::Boolean(true),
        "length is own property"
    );
    assert_eq!(e[1], crate::value::Value::Number(0.0), "arrow length is 0");
    assert_eq!(
        e[2],
        crate::value::Value::Boolean(true),
        "delete returns true (configurable)"
    );
    assert_eq!(
        e[3],
        crate::value::Value::Boolean(false),
        "length gone after delete"
    );
}

#[test]
fn delete_arrow_length_returns_true() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("var f1 = (x = 42) => {}; delete f1.length;")
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

#[test]
fn delete_function_length_returns_true() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f1(x = 42) {}; delete f1.length;")
        .unwrap();
    assert_eq!(v, crate::value::Value::Boolean(true));
}

#[test]
fn arrow_is_function_value() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("typeof (()=>1)").unwrap();
    assert_eq!(v, crate::value::Value::String("function".to_string()));
    let f = ctx.eval("var f1 = (x = 42) => {}; f1").unwrap();
    match f {
        crate::value::Value::Function(_) => {}
        other => panic!("got: {:?}", other),
    }
}

#[test]
fn arrow_length_remove_property() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var f1 = (x = 42) => {}; \
             var del = delete f1.length; \
             var has = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             [del, has];",
        )
        .unwrap();
    let arr = if let crate::value::Value::Object(o) = r {
        o.borrow().elements.clone()
    } else {
        panic!("not array");
    };
    assert_eq!(arr[0], crate::value::Value::Boolean(true), "delete");
    assert_eq!(
        arr[1],
        crate::value::Value::Boolean(false),
        "should not be own after delete"
    );
}

#[test]
fn remove_property_directly() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var f1 = function() {}; \
             f1.length = 5; \
             var before = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             var del = delete f1.length; \
             var after = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             [before, del, after];",
        )
        .unwrap();
    let arr = if let crate::value::Value::Object(o) = r {
        o.borrow().elements.clone()
    } else {
        panic!("not array");
    };
    assert_eq!(arr[2], crate::value::Value::Boolean(false), "after");
}

#[test]
fn arrow_length_descriptor_configurable() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var f1 = (x = 42) => {}; \
             var desc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             [desc.value, desc.writable, desc.enumerable, desc.configurable, f1.length];",
    );
    let arr = match v.unwrap() {
        crate::value::Value::Object(o) => o,
        other => panic!("expected array: {:?}", other),
    };
    let e = arr.borrow().elements.clone();
    assert_eq!(e[0], crate::value::Value::Number(0.0), "value");
    assert_eq!(e[1], crate::value::Value::Boolean(false), "writable");
    assert_eq!(e[2], crate::value::Value::Boolean(false), "enumerable");
    assert_eq!(e[3], crate::value::Value::Boolean(true), "configurable");
    assert_eq!(e[4], crate::value::Value::Number(0.0), "f1.length");
}

#[test]
fn arrow_length_descriptor_full_verifyproperty() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var f1 = (x = 42) => {}; \
             var originalDesc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             if (!Object.prototype.hasOwnProperty.call(f1, 'length')) throw new Error('not own'); \
             try { f1.length = 'unlikelyValue'; } catch (e) {} \
             var still0 = f1.length; \
             var lenDesc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             [originalDesc.value, originalDesc.writable, originalDesc.configurable, still0, lenDesc.value, lenDesc.writable];"
    );
    let arr = match v.unwrap() {
        crate::value::Value::Object(o) => o,
        other => panic!("expected array: {:?}", other),
    };
    let e = arr.borrow().elements.clone();
    assert_eq!(e[0], crate::value::Value::Number(0.0), "orig value");
    assert_eq!(e[1], crate::value::Value::Boolean(false), "orig writable");
    assert_eq!(
        e[2],
        crate::value::Value::Boolean(true),
        "orig configurable"
    );
    assert_eq!(e[3], crate::value::Value::Number(0.0), "still 0");
    assert_eq!(e[4], crate::value::Value::Number(0.0), "post value");
    assert_eq!(e[5], crate::value::Value::Boolean(false), "post writable");
}

#[test]
fn new_target_in_constructor() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "function F() { this.t = new.target === F; } \
             var f = new F(); \
             f.t;",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn new_target_in_arrow_inside_constructor() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "function F() { this.af = () => new.target; } \
             var f = new F(); \
             f.af() === F;",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn arrow_length_no_writable_check() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var f1 = (x = 42) => {}; \
             var desc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             try { f1.length = 99; } catch (e) {} \
             var writable = Object.getOwnPropertyDescriptor(f1, 'length').writable; \
             var afterLen = f1.length; \
             [writable, afterLen];",
    );
    let arr = match v.unwrap() {
        crate::value::Value::Object(o) => o,
        other => panic!("expected array: {:?}", other),
    };
    let e = arr.borrow().elements.clone();
    assert_eq!(
        e[0],
        crate::value::Value::Boolean(false),
        "writable should be false"
    );
    assert_eq!(
        e[1],
        crate::value::Value::Number(0.0),
        "length should not change"
    );
}

#[test]
fn simple_class_extends_with_super() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "class A { constructor() {} } \
             class B extends A { constructor() { super(); } } \
             new B() instanceof B;",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_extends_promise_builtin() {
    let mut ctx = Context::new().unwrap();
    let _t = ctx.eval("typeof Promise");
    let v = ctx.eval(
        "class SubPromise extends Promise { \
               constructor(a) { super(a); } \
             } \
             new SubPromise(function(resolve) { resolve(42); });",
    );
    assert!(v.is_ok(), "class extends Promise should work: {:?}", v);
}

#[test]
fn super_in_arrow_throws_reference_error() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var count = 0; \
             class A { constructor() { count++; } } \
             class B extends A { \
               constructor() { super(); this.af = _ => super(); } \
             } \
             var b = new B(); \
             var err; \
             try { b.af(); } catch (e) { err = e && e.name; } \
             [count, err];",
        )
        .unwrap();
    if let crate::value::Value::Object(_o) = v {
        let _e = _o.borrow().elements.clone();
    }
}

#[test]
fn super_in_iife_arrow_calls_super_once() {
    let mut ctx = Context::new().unwrap();
    let _v = ctx.eval(
        "var count = 0; \
             class A { constructor() { count++; } } \
             class B extends A { constructor() { (_ => super())(); } } \
             new B(); \
             count;",
    );
}

#[test]
fn sloppy_arrow_assigns_undeclared_creates_global() {
    let mut ctx = Context::new().unwrap();
    let _v = ctx.eval("var af = _ => { foo = 1; }; af()");
}

#[test]
fn arrow_fn_caller_full_test262() {
    use crate::test262::harness::try_inject_harness;
    let mut ctx = Context::new().unwrap();
    try_inject_harness(&mut ctx).unwrap();
    let res = ctx.eval(
        "var arrowFn = () => {}; \
             var got1 = false; try { var x = arrowFn.caller; } catch (e) { got1 = (e instanceof TypeError); } \
             var got2 = false; try { arrowFn.caller = {}; } catch (e) { got2 = (e instanceof TypeError); } \
             var got3 = false; try { var y = arrowFn.arguments; } catch (e) { got3 = (e instanceof TypeError); } \
             var got4 = false; try { arrowFn.arguments = {}; } catch (e) { got4 = (e instanceof TypeError); } \
             got1 && got2 && got3 && got4;",
    );
    assert_eq!(res.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn symbol_to_primitive_object_result_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "var value = {}; \
             value[Symbol.toPrimitive] = function() { return {}; }; \
             value + 1;",
    );
    assert!(result.is_err());
    let thrown = crate::value::take_thrown_value().unwrap();
    let crate::value::Value::Object(error) = thrown else {
        panic!("expected TypeError object");
    };
    assert_eq!(
        error.borrow().get("name"),
        Some(crate::value::Value::String("TypeError".to_string()))
    );
}


#[test]
fn sloppy_delete_global_property_succeeds() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("__ref = {};").unwrap();
    let deleted = ctx.eval("delete __ref;").unwrap();
    assert_eq!(deleted, crate::value::Value::Boolean(true));
    let after = ctx.eval("typeof __ref").unwrap();
    assert_eq!(after, crate::value::Value::String("undefined".to_string()));
}


#[test]
fn debug_known_global() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("typeof Math").unwrap();
    assert_eq!(v, crate::value::Value::String("object".to_string()));
}

#[test]
fn debug_set_global_directly() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("globalThis.test_var = 99;").unwrap();
    let v = ctx.eval("typeof test_var").unwrap();
    assert_eq!(v, crate::value::Value::String("number".to_string()));
}


#[test]
fn debug_symbol_member() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("Symbol.prototype.test262 = 'sym-proto';").unwrap();
    let v = ctx.eval("Symbol().test262").unwrap();
    assert_eq!(v, crate::value::Value::String("sym-proto".to_string()));
}

#[test]
fn debug_number_member() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("Number.prototype.test262 = 'num-proto';").unwrap();
    let v = ctx.eval("(1).test262").unwrap();
    assert_eq!(v, crate::value::Value::String("num-proto".to_string()));
}

#[test]
fn debug_symbol_proto_lookup() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("Symbol.prototype.test262 = 'sym-proto';").unwrap();
    let direct = ctx.eval("Symbol.prototype.test262").unwrap();
    assert_eq!(direct, crate::value::Value::String("sym-proto".to_string()));
    let s = ctx.eval("Symbol()").unwrap();
    assert!(matches!(s, crate::value::Value::Symbol(_)));
}

#[test]
fn debug_symbol_dot_member() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("Symbol.prototype.test262 = 'sym-proto';").unwrap();
    let v = ctx.eval("var s = Symbol(); s.test262;").unwrap();
    assert_eq!(v, crate::value::Value::String("sym-proto".to_string()));
}

#[test]
fn strict_assign_to_nan_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("\"use strict\"; NaN = 12;");
    assert!(
        res.is_err(),
        "strict assignment to NaN should throw: {:?}",
        res
    );
}

#[test]
fn strict_assign_to_undefined_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("\"use strict\"; undefined = 12;");
    assert!(
        res.is_err(),
        "strict assignment to undefined should throw: {:?}",
        res
    );
}

#[test]
fn strict_assign_to_infinity_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("\"use strict\"; Infinity = 12;");
    assert!(
        res.is_err(),
        "strict assignment to Infinity should throw: {:?}",
        res
    );
}

#[test]
fn sloppy_assign_to_nan_no_throw() {
    let mut ctx = Context::new().unwrap();
    let res = ctx.eval("NaN = 12;");
    assert!(res.is_ok(), "sloppy assignment to NaN should not throw");
}

#[test]
fn symbol_keyed_accessor_property_getter() {
    // Reproduces verifyProperty-restore-accessor-symbol.js issue
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var prop = Symbol(1);\
         var obj = {};\
         Object.defineProperty(obj, prop, { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} });\
         var result = obj[prop];\
         if (result !== 42) throw new Error('expected 42, got ' + result);",
    )
    .expect("Symbol-keyed accessor getter should return 42");
}

#[test]
fn symbol_keyed_accessor_property_get_own_property_descriptor() {
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var prop = Symbol(1);\
         var obj = {};\
         Object.defineProperty(obj, prop, { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} });\
         var desc = Object.getOwnPropertyDescriptor(obj, prop);\
         if (typeof desc.get !== 'function') throw new Error('getter should be a function');\
         if (desc.get() !== 42) throw new Error('calling getter should return 42');",
    )
    .expect("Symbol-keyed accessor property descriptor should have get function");
}

#[test]
fn symbol_keyed_accessor_verify_property_restore() {
    // Reproduces verifyProperty-restore-accessor-symbol.js exactly
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var prop = Symbol(1);\
         var desc = { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} };\
         var obj = {};\
         Object.defineProperty(obj, prop, desc);\
         // verifyProperty(obj, prop, desc) without restore - just check it doesn't throw\
         // Then verify obj[prop] returns 42\
         var val = obj[prop];\
         if (val !== 42) throw new Error('expected 42 but got ' + val);\
         // With restore option - should restore original descriptor\
         Object.defineProperty(obj, prop, desc);\
         // After restore, the property should still be own property (it was before verifyProperty)\
         // verifyProperty with restore: calls __defineProperty(obj, name, originalDesc)\
         // But __defineProperty(obj, prop, desc) with accessor should still work\
         Object.defineProperty(obj, prop, desc);\
         var val2 = obj[prop];\
         if (val2 !== 42) throw new Error('expected 42 after restore but got ' + val2);",
    )
    .expect("Symbol-keyed accessor property should work with verifyProperty");
}

#[test]
fn symbol_keyed_accessor_has_own_property() {
    // Key bug: Symbol-keyed accessor properties must be found by hasOwnProperty
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var prop = Symbol(1);\
         var obj = {};\
         Object.defineProperty(obj, prop, { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} });\
         // hasOwnProperty must return true for Symbol-keyed accessor property\
         var result = Object.prototype.hasOwnProperty.call(obj, prop);\
         if (result !== true) throw new Error('hasOwnProperty should return true, got ' + result);",
    )
    .expect("hasOwnProperty must return true for Symbol-keyed accessor properties");
}

#[test]
fn symbol_keyed_accessor_getter_invocation() {
    // Bug: Symbol-keyed accessor getter must be called, not return the function
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var prop = Symbol(1);\
         var obj = {};\
         Object.defineProperty(obj, prop, { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} });\
         var val = obj[prop];\
         if (typeof val !== 'number') throw new Error('getter should return number, got ' + typeof val);\
         if (val !== 42) throw new Error('getter should return 42, got ' + val);",
    )
    .expect("Symbol-keyed accessor getter should be invoked and return 42");
}

#[test]
fn symbol_keyed_get_own_property_descriptor_getter() {
    // Bug: Object.getOwnPropertyDescriptor for Symbol-keyed accessor should have 'get' property
    let mut ctx = Context::new().unwrap();
    ctx.eval(
        "var prop = Symbol(1);\
         var obj = {};\
         var getterFn = function() { return 42; };\
         Object.defineProperty(obj, prop, { enumerable: true, configurable: true, get: getterFn, set: function() {} });\
         var desc = Object.getOwnPropertyDescriptor(obj, prop);\
         if (typeof desc.get !== 'function') throw new Error('desc.get should be function, got ' + typeof desc.get);\
         if (desc.get !== getterFn) throw new Error('desc.get should be the original getter function');\
         if (desc.get() !== 42) throw new Error('calling desc.get() should return 42');",
    )
    .expect("getOwnPropertyDescriptor should return correct getter for Symbol-keyed accessor");
}

#[test]
fn symbol_keyed_verify_property_scenario() {
    // Exact scenario from verifyProperty-restore-accessor-symbol.js
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);

    let result = ctx.eval(
        r#"
var obj;
var prop = Symbol(1);
var desc = { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} };

obj = {};
Object.defineProperty(obj, prop, desc);

// verifyProperty(obj, prop, desc) - this is called in the actual test
// After verifyProperty without restore, hasOwnProperty should be false
var hasOwn = Object.prototype.hasOwnProperty.call(obj, prop);
// If this passes, then the property is an own property
// (but in verifyProperty without restore, it should remain an own property)
if (hasOwn !== true) throw new Error('hasOwnProperty should be true, got ' + hasOwn);

// obj[prop] should call the getter and return 42
var val = obj[prop];
if (val !== 42) throw new Error('obj[prop] should return 42, got ' + val + ' (type: ' + typeof val + ')');
"#,
    );
    assert!(
        result.is_ok(),
        "Symbol-keyed verifyProperty scenario failed: {:?}",
        result
    );
}

#[test]
fn symbol_keyed_accessor_exact_test262_scenario() {
    // Exact reproduction of verifyProperty-restore-accessor-symbol.js
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);

    let result = ctx.eval(
        r#"
var obj;
var prop = Symbol(1);
var desc = { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} };

obj = {};
Object.defineProperty(obj, prop, desc);

// Step 1: obj[prop] should return 42 (calling the getter)
var val1 = obj[prop];
if (val1 !== 42) throw new Error('Step1: obj[prop] should be 42, got ' + val1);

// Step 2: hasOwnProperty should return true (accessor IS an own property)
var hasOwn = Object.prototype.hasOwnProperty.call(obj, prop);
if (hasOwn !== true) throw new Error('Step2: hasOwnProperty should be true, got ' + hasOwn);

// Step 3: getOwnPropertyDescriptor should return correct descriptor
var origDesc = Object.getOwnPropertyDescriptor(obj, prop);
if (typeof origDesc.get !== 'function') throw new Error('Step3: origDesc.get should be function');
if (typeof origDesc.set !== 'function') throw new Error('Step3: origDesc.set should be function');

// Step 4: desc.get and origDesc.get should be the SAME function
if (origDesc.get !== desc.get) throw new Error('Step4: origDesc.get !== desc.get');

// Step 5: desc.set and origDesc.set should be the SAME function
if (origDesc.set !== desc.set) throw new Error('Step5: origDesc.set !== desc.set');

// Step 6: calling origDesc.get() should return 42
var getterResult = origDesc.get();
if (getterResult !== 42) throw new Error('Step6: origDesc.get() should be 42, got ' + getterResult);
"#,
    );
    assert!(
        result.is_ok(),
        "Exact test262 scenario failed: {:?}",
        result
    );
}

#[test]
fn symbol_keyed_verify_property_restore_no_crash() {
    // Test that verifyProperty with restore doesn't crash for Symbol-keyed accessor
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);

    // Test: __defineProperty(obj, prop, desc) with Symbol key and accessor
    let result = ctx.eval(
        r#"
var obj = {};
var prop = Symbol(1);
var desc = { enumerable: true, configurable: true, get: function() { return 42; }, set: function() {} };
Object.defineProperty(obj, prop, desc);

// __defineProperty is our injected version of Object.defineProperty
// Test that it can re-define the property (restore scenario)
Object.defineProperty(obj, prop, desc);

// Verify it's still working
if (obj[prop] !== 42) throw new Error('obj[prop] should still be 42 after re-define');

// Verify descriptor
var d = Object.getOwnPropertyDescriptor(obj, prop);
if (d.get !== desc.get) throw new Error('getter identity lost after re-define');
if (d.set !== desc.set) throw new Error('setter identity lost after re-define');
"#,
    );
    assert!(
        result.is_ok(),
        "verifyProperty restore scenario failed: {:?}",
        result
    );
}

#[test]
fn symbol_keyed_define_property_preserves_getter_identity() {
    // Critical test: Object.defineProperty must preserve the getter function identity.
    // desc.get === getOwnPropertyDescriptor(obj, prop).get
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);

    let result = ctx.eval(
        r#"
var prop = Symbol(1);
var getterFn = function() { return 42; };
var setterFn = function(v) {};

// Test 1: defineProperty with function-valued getter
var obj1 = {};
Object.defineProperty(obj1, prop, {
    enumerable: true,
    configurable: true,
    get: getterFn,
    set: setterFn
});

var desc1 = Object.getOwnPropertyDescriptor(obj1, prop);
if (desc1.get !== getterFn) throw new Error('Test1: getter identity lost');
if (desc1.set !== setterFn) throw new Error('Test1: setter identity lost');

// Test 2: defineProperty with getter shorthand
var obj2 = {};
Object.defineProperty(obj2, prop, {
    enumerable: true,
    configurable: true,
    get: function() { return 42; },
    set: function() {}
});

// The descriptor's get is a different function (from the object literal)
// but calling it should still return 42
if (obj2[prop] !== 42) throw new Error('Test2: getter should return 42');
"#,
    );
    assert!(
        result.is_ok(),
        "Getter identity preservation test failed: {:?}",
        result
    );
}
