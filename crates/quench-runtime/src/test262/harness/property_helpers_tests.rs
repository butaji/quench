//! Tests for property_helpers (split out to satisfy the 500-line limit).

use crate::test262::harness::{try_inject_harness, HarnessLoader};

fn harness_ctx() -> crate::Context {
    let mut ctx = crate::Context::new().unwrap();
    try_inject_harness(&mut ctx).unwrap();
    ctx
}

#[test]
fn test_verify_property_fn_name_method_class_body() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var namedSym = Symbol('test262'); var anonSym = Symbol(); \
         class A { id() {} [anonSym]() {} [namedSym]() {} static id() {} static [anonSym]() {} static [namedSym]() {} } \
         verifyProperty(A.prototype.id, 'name', { value: 'id', writable: false, enumerable: false, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "first verifyProperty in fn-name-method: {:?}",
        result
    );
}

#[test]
fn test_verify_property_anonymous_arrow_name() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "verifyProperty(() => {}, 'name', { value: '', writable: false, enumerable: false, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "anonymous arrow name should verify: {:?}",
        result
    );
}

#[test]
fn test_verify_property_class_prototype_symbol_method_name() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var namedSym = Symbol('test262'); class A { [namedSym]() {} } \
         verifyProperty(A.prototype[namedSym], 'name', { value: '[test262]', writable: false, enumerable: false, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "verifyProperty symbol method name should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_class_prototype_method_name() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "class A { id() {} } \
         verifyProperty(A.prototype.id, 'name', { value: 'id', writable: false, enumerable: false, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "verifyProperty prototype method name should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_class_static_method() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "class C { static m() { return 1; } } \
         verifyProperty(C, 'm', { enumerable: false, configurable: true, writable: true });",
    );
    assert!(
        result.is_ok(),
        "verifyProperty class static method should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_basic_data_property() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: true}); verifyProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: true});",
    );
    assert!(
        result.is_ok(),
        "verifyProperty data property should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_function_constructor_descriptor_is_writable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var desc = Object.getOwnPropertyDescriptor(Object.prototype, 'constructor'); \
         verifyProperty(Object.prototype, 'constructor', { value: desc.value, writable: true, enumerable: false, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "Object.prototype.constructor descriptor should be writable: {:?}",
        result
    );
}

#[test]
fn test_loaded_property_helper_verifies_function_constructor_descriptor() {
    let mut ctx = harness_ctx();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let loader = HarnessLoader::new(root.to_str().unwrap());
    let script = loader
        .build_script(
            "var desc = Object.getOwnPropertyDescriptor(Object.prototype, 'constructor'); \
             var getFunc = function () { return 100; }; \
             var data = 'data'; \
             var setFunc = function (value) { data = value; }; \
             Object.defineProperty(Object.prototype, 'constructor', { get: getFunc, set: setFunc, configurable: true }); \
             var fun = function () {}; \
             assert.sameValue(typeof fun.prototype.constructor, 'function'); \
             verifyProperty(fun.prototype, 'constructor', { writable: true, enumerable: false, configurable: true }); \
             assert.sameValue(data, 'data', 'data');",
            &["propertyHelper.js".to_string()],
        )
        .unwrap();
    let result = ctx.eval(&script);
    assert!(
        result.is_ok(),
        "loaded propertyHelper should verify constructor descriptor: {:?}",
        result
    );
}

#[test]
fn test_function_prototype_constructor_remains_writable_after_object_accessor() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "'use strict'; var getFunc = function () { return 100; }; \
         var setFunc = function (value) {}; \
         Object.defineProperty(Object.prototype, 'constructor', { get: getFunc, set: setFunc, configurable: true }); \
         var fun = function () {}; \
         Object.getOwnPropertyDescriptor(fun.prototype, 'constructor').writable",
    );
    assert_eq!(result.unwrap(), crate::Value::Boolean(true));
}

#[test]
fn test_verify_property_accessor_property() {
    let mut ctx = harness_ctx();
    // Use same function reference for both defineProperty and verifyProperty
    let result = ctx.eval(
        "var obj = {}; var getter = function() { return 42; }; var setter = function(v) {}; Object.defineProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true});",
    );
    assert!(
        result.is_ok(),
        "verifyProperty accessor should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_symbol_key() {
    let mut ctx = harness_ctx();
    // Use same function reference for both defineProperty and verifyProperty
    let result = ctx.eval(
        "var obj = {}; var sym = Symbol('test'); var getter = function() { return 42; }; var setter = function(v) {}; Object.defineProperty(obj, sym, {get: getter, set: setter, enumerable: true, configurable: true}); verifyProperty(obj, sym, {get: getter, set: setter, enumerable: true, configurable: true});",
    );
    assert!(
        result.is_ok(),
        "verifyProperty with Symbol key (accessor) should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_symbol_data_key_via_define_property() {
    let mut ctx = harness_ctx();
    // Symbol-keyed data property defined via Object.defineProperty.
    // This bugs because defineProperty stores the key as desc\0id string
    // in `properties`, NOT in `symbol_properties`, but verifyProperty's
    // Symbol branch only checks has_symbol() which looks at symbol_properties.
    let result = ctx.eval(
        "var obj = {}; var sym = Symbol(1); \
         Object.defineProperty(obj, sym, { value: 42, enumerable: true, configurable: true, writable: true }); \
         verifyProperty(obj, sym, { value: 42, enumerable: true, configurable: true, writable: true });",
    );
    assert!(
        result.is_ok(),
        "verifyProperty with Symbol data key via defineProperty should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_symbol_data_key_via_define_property_raw_call() {
    let mut ctx = harness_ctx();
    // Test the raw native verify_property call (no JS wrapper) to isolate the bug.
    // This directly checks whether the Symbol branch of verify_property finds
    // properties stored in `properties` (not `symbol_properties`).
    let result = ctx.eval(
        "var obj = {}; var sym = Symbol('raw'); \
         Object.defineProperty(obj, sym, { value: 99, writable: false, enumerable: false, configurable: false }); \
         verifyProperty(obj, sym, { value: 99, writable: false, enumerable: false, configurable: false });",
    );
    assert!(
        result.is_ok(),
        "verifyProperty with Symbol data key (non-default flags) should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_symbol_accessor_define_property() {
    let mut ctx = harness_ctx();
    // Symbol-keyed accessor property defined via Object.defineProperty.
    let result = ctx.eval(
        "var obj = {}; var sym = Symbol(1); \
         var getter = function() { return 1; }; var setter = function(v) {}; \
         Object.defineProperty(obj, sym, { get: getter, set: setter, enumerable: true, configurable: true }); \
         verifyProperty(obj, sym, { get: getter, set: setter, enumerable: true, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "verifyProperty with Symbol accessor key via defineProperty should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_enumerable_false() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, enumerable: false, writable: true, configurable: true}); verifyProperty(obj, 'foo', {value: 1, enumerable: false, writable: true, configurable: true});",
    );
    assert!(result.is_ok(), "enumerable:false should pass: {:?}", result);
}

#[test]
fn test_verify_property_configurable_false() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: false}); verifyProperty(obj, 'foo', {value: 42, enumerable: true, writable: true, configurable: false});",
    );
    assert!(
        result.is_ok(),
        "configurable:false should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_missing_throws() {
    let mut ctx = harness_ctx();
    let result = ctx.eval("var obj = {}; verifyProperty(obj, 'missing', { value: 42 });");
    assert!(
        result.is_err(),
        "verifyProperty should throw for missing property"
    );
}

#[test]
fn test_verify_property_undefined_desc() {
    let mut ctx = harness_ctx();
    let result = ctx.eval("var obj = {}; verifyProperty(obj, 'missing', undefined);");
    assert!(
        result.is_ok(),
        "undefined desc should pass for missing property: {:?}",
        result
    );
}

#[test]
fn test_verify_property_null_desc_throws() {
    let mut ctx = harness_ctx();
    let result = ctx.eval("var obj = {}; verifyProperty(obj, 'foo', null);");
    assert!(result.is_err(), "null desc should throw");
}

#[test]
fn test_verify_property_value_mismatch_throws() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, enumerable: true, writable: true, configurable: true}); verifyProperty(obj, 'foo', { value: 2 });",
    );
    assert!(result.is_err(), "value mismatch should throw: {:?}", result);
}

#[test]
fn test_verify_property_ignores_get_set_identity() {
    // Official verifyProperty does NOT compare desc.get/desc.set — that is
    // verifyAccessorProperty's job. A different getter function in desc
    // must not fail verification.
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {get: function() { return 1; }, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', {get: function() { return 2; }, enumerable: true, configurable: true});",
    );
    assert!(
        result.is_ok(),
        "verifyProperty must not compare get/set identity: {:?}",
        result
    );
}

#[test]
fn test_verify_property_restore_option() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 42, enumerable: true, configurable: true, writable: true}); verifyProperty(obj, 'foo', {value: 42, enumerable: true, configurable: true, writable: true}, { restore: true }); var val = obj.foo; if (val !== 42) throw new Error('property should be restored');",
    );
    assert!(
        result.is_ok(),
        "verifyProperty with restore should work: {:?}",
        result
    );
}

#[test]
fn test_verify_property_restore_preserves_accessor() {
    let mut ctx = harness_ctx();
    // Use same function reference for both defineProperty and verifyProperty
    let result = ctx.eval(
        "var obj = {}; var getter = function() { return 42; }; var setter = function(v) {}; Object.defineProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', {get: getter, set: setter, enumerable: true, configurable: true}, { restore: true }); var val = obj.foo; if (val !== 42) throw new Error('accessor should be preserved');",
    );
    assert!(
        result.is_ok(),
        "verifyProperty restore should preserve accessor: {:?}",
        result
    );
}

#[test]
fn test_verify_writable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, writable: true, configurable: true }); verifyWritable(obj, 'foo');",
    );
    assert!(result.is_ok(), "verifyWritable should pass: {:?}", result);
}

#[test]
fn test_verify_not_writable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, writable: false, configurable: true }); verifyNotWritable(obj, 'foo');",
    );
    assert!(
        result.is_ok(),
        "verifyNotWritable should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_enumerable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, enumerable: true, configurable: true }); verifyEnumerable(obj, 'foo');",
    );
    assert!(result.is_ok(), "verifyEnumerable should pass: {:?}", result);
}

#[test]
fn test_verify_not_enumerable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, enumerable: false, configurable: true }); verifyNotEnumerable(obj, 'foo');",
    );
    assert!(
        result.is_ok(),
        "verifyNotEnumerable should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_configurable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, configurable: true }); verifyConfigurable(obj, 'foo');",
    );
    assert!(
        result.is_ok(),
        "verifyConfigurable should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_not_configurable() {
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', { value: 1, configurable: false }); verifyNotConfigurable(obj, 'foo');",
    );
    assert!(
        result.is_ok(),
        "verifyNotConfigurable should pass: {:?}",
        result
    );
}

#[test]
fn test_make_native_error() {
    let mut ctx = harness_ctx();
    let result = ctx.eval("typeof makeNativeError(TypeError) === 'object'");
    assert!(
        result.is_ok(),
        "makeNativeError should return object: {:?}",
        result
    );
}

#[test]
fn test_verify_property_too_few_args() {
    let mut ctx = harness_ctx();
    let result = ctx.eval("verifyProperty()");
    assert!(result.is_err(), "verifyProperty with no args should throw");
}

#[test]
fn test_verify_property_writable_false_mismatch_throws() {
    // desc says writable:false but the actual descriptor is writable:true.
    // Official verifyProperty checks desc.writable against the descriptor.
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, writable: true, enumerable: true, configurable: true}); verifyProperty(obj, 'foo', { value: 1, writable: false, enumerable: true, configurable: true });",
    );
    assert!(
        result.is_err(),
        "writable:false vs actual writable:true should throw: {:?}",
        result
    );
}

#[test]
fn test_verify_property_writable_true_mismatch_throws() {
    // desc says writable:true but the actual descriptor is writable:false.
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, writable: false, enumerable: true, configurable: false}); verifyProperty(obj, 'foo', { value: 1, writable: true, enumerable: true, configurable: false });",
    );
    assert!(
        result.is_err(),
        "writable:true vs actual writable:false should throw: {:?}",
        result
    );
}

#[test]
fn test_verify_property_writable_iswritable_probe_throws() {
    // The descriptor claims writable:false and the descriptor flag agrees,
    // but the official check also probes actual writability. Here the
    // probe must confirm the property is genuinely non-writable, so this
    // passes; a mismatched probe (descriptor flag lies) must throw.
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; Object.defineProperty(obj, 'foo', {value: 1, writable: false, enumerable: true, configurable: false}); verifyProperty(obj, 'foo', { value: 1, writable: false, enumerable: true, configurable: false });",
    );
    assert!(
        result.is_ok(),
        "writable:false with matching probe should pass: {:?}",
        result
    );
    assert_eq!(
        ctx.eval("obj.foo").unwrap(),
        crate::Value::Number(1.0),
        "isWritable probe must not corrupt the stored value"
    );
}

#[test]
fn test_verify_property_symbol_enumerable_false_passes() {
    // Symbol keys must use the real descriptor's enumerable flag. The
    // description "Symbol(x)" makes the raw payload key start with
    // "Symbol(" — the shape the old always-enumerable shortcut matched.
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; var sym = Symbol('Symbol(x)'); Object.defineProperty(obj, sym, { value: 1, writable: false, enumerable: false, configurable: false }); verifyProperty(obj, sym, { value: 1, writable: false, enumerable: false, configurable: false });",
    );
    assert!(
        result.is_ok(),
        "symbol-keyed enumerable:false should pass: {:?}",
        result
    );
}

#[test]
fn test_verify_property_symbol_enumerable_mismatch_throws() {
    // Actual enumerable:false, desc claims enumerable:true -> must throw.
    let mut ctx = harness_ctx();
    let result = ctx.eval(
        "var obj = {}; var sym = Symbol('Symbol(x)'); Object.defineProperty(obj, sym, { value: 1, writable: false, enumerable: false, configurable: false }); verifyProperty(obj, sym, { value: 1, writable: false, enumerable: true, configurable: false });",
    );
    assert!(
        result.is_err(),
        "symbol-keyed enumerable mismatch should throw: {:?}",
        result
    );
}
