//! Tests for Object built-in

use crate::Context;

fn eval(src: &str) -> Result<crate::Value, crate::value::JsError> {
    Context::new().unwrap().eval(src)
}

/// super(42) should NOT throw — the Object constructor called via
/// super() must handle primitive arguments without error.
#[test]
fn object_super_with_number_arg_ok() {
    let r = eval(
        "class MyObj extends Object { \
         constructor() { super(42); } \
         } \
         new MyObj()",
    );
    assert!(r.is_ok(), "super(42) should succeed: {:?}", r.err());
    let v = r.unwrap();
    assert!(matches!(v, crate::Value::Object(_)));
}

/// super() (no argument) should work.
#[test]
fn object_super_no_arg_ok() {
    let r = eval(
        "class MyObj extends Object { \
         constructor() { super(); } \
         } \
         new MyObj()",
    );
    assert!(r.is_ok(), "super() should succeed: {:?}", r.err());
}

/// super('hello') should NOT throw.
#[test]
fn object_super_with_string_arg_ok() {
    let r = eval(
        "class MyObj extends Object { \
         constructor() { super('hello'); } \
         } \
         new MyObj()",
    );
    assert!(r.is_ok(), "super('hello') should succeed: {:?}", r.err());
}

/// A derived class constructor that returns a non-object (42) should
/// throw TypeError per ES §9.2.2 [[Construct]] step 13.b.
#[test]
fn derived_constructor_returns_primitive_throws_typeerror() {
    let r = eval(
        "class Obj extends Object { \
         constructor() { return 42; } \
         } \
         new Obj()",
    );
    assert!(r.is_err(), "constructor returning 42 should throw");
    let err = r.unwrap_err().to_string();
    assert!(
        err.contains("TypeError"),
        "error should be TypeError, got: {err}"
    );
}

/// A base class constructor that returns a primitive ignores that result.
#[test]
fn base_constructor_returns_primitive_throws_typeerror() {
    let r = eval(
        "class Base { \
         constructor() { return 42; } \
         } \
         new Base()",
    );
    assert!(matches!(r, Ok(crate::Value::Object(_))));
}

#[test]
fn value_of_boxed_primitive_works_in_arithmetic() {
    // This was the original crash: Object(2n) + 1n caused stack overflow
    // because valueOf returned `this` (the Object) instead of the boxed BigInt.
    // The fix: valueOf checks for _value property and returns it.
    let r = eval("'ok'").unwrap();
    assert_eq!(r, crate::Value::String("ok".to_string()), "sanity check");

    // First verify Object(2n) doesn't crash
    let r2 = eval("typeof Object(2n)").unwrap();
    assert_eq!(
        r2,
        crate::Value::String("object".to_string()),
        "typeof Object(2n) should be object"
    );
}

#[test]
fn value_of_boxed_number_returns_number() {
    let r = eval("Object(42).valueOf() === 42").unwrap();
    assert_eq!(r, crate::Value::Boolean(true), "Object(42).valueOf() = 42");
}

#[test]
fn value_of_boxed_string_returns_string() {
    let r = eval("Object('hello').valueOf() === 'hello'").unwrap();
    assert_eq!(
        r,
        crate::Value::Boolean(true),
        "Object('hello').valueOf() = 'hello'"
    );
}

#[test]
fn object_constructor_length_is_one() {
    assert_eq!(eval("Object.length").unwrap(), crate::Value::Number(1.0));
}

#[test]
fn object_assign_boxes_string_target() {
    assert_eq!(
        eval("[typeof Object.assign('a'), Object.assign('a').valueOf()].join('|')").unwrap(),
        crate::Value::String("object|a".into())
    );
}

#[test]
fn object_assign_boxes_string_sources() {
    assert_eq!(
        eval("Object.getOwnPropertyNames(Object.assign(12, 'aaa', 'bb2b', '1c')).length").unwrap(),
        crate::Value::Number(4.0)
    );
}

#[test]
fn object_assign_boxes_boolean_target() {
    assert_eq!(
        eval("typeof Object.assign(true)").unwrap(),
        crate::Value::String("object".into())
    );
}

#[test]
fn object_assign_boxes_symbol_target() {
    assert_eq!(
        eval("Object.assign(Symbol('foo')).toString()").unwrap(),
        crate::Value::String("Symbol(foo)".into())
    );
}

#[test]
fn reflect_set_rejects_string_index() {
    assert_eq!(
        eval("Reflect.set(__ops__.ToObject('a'), '0', 'b')").unwrap(),
        crate::Value::Boolean(false)
    );
}

#[test]
fn define_property_rejects_replacing_string_index() {
    let mut ctx = crate::Context::new().unwrap();
    assert!(ctx
        .eval("Object.defineProperty(__ops__.ToObject('a'), '0', {value: 'b'})")
        .is_err());
}

#[test]
fn object_assign_propagates_source_descriptor_error() {
    let result = eval(
        "var source = new Proxy({attr: null}, {getOwnPropertyDescriptor: function() { throw new Test262Error(); }}); Object.assign({}, source)",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Test262Error"));
}

#[test]
fn object_assign_updates_array_elements() {
    assert_eq!(
        eval("Object.assign([1, 8, 9], [, , 3]).join(',')").unwrap(),
        crate::Value::String("1,8,3".into())
    );
}

#[test]
fn object_assign_skips_sparse_array_holes() {
    assert_eq!(
        eval("var target = [7, 8, 9]; Object.assign(target, [1]).join(',')").unwrap(),
        crate::Value::String("1,8,9".into())
    );
    assert_eq!(
        eval("var source = []; source[2] = 3; Object.keys(source).join(',')").unwrap(),
        crate::Value::String("2".into())
    );
    assert_eq!(
        eval("var target = [7, 8, 9]; var source = []; source[2] = 3; Object.assign(target, source).join(',')").unwrap(),
        crate::Value::String("7,8,3".into())
    );
}

#[test]
fn object_assign_calls_setter_on_frozen_target() {
    assert_eq!(
        eval("var value = 0; var target = {}; Object.defineProperty(target, 'x', {set: function(v) { value = v; }, configurable: false}); Object.freeze(target); Object.assign(target, {x: 3}); value").unwrap(),
        crate::Value::Number(3.0)
    );
}

#[test]
fn object_assign_calls_object_literal_setter_on_frozen_target() {
    assert_eq!(
        eval("var value = 1; var target = {set foo(v) { value = v; }}; Object.freeze(target); Object.assign(target, {foo: 2}); value").unwrap(),
        crate::Value::Number(2.0)
    );
}

#[test]
fn object_assign_calls_symbol_setter_on_frozen_target() {
    let mut context = Context::new().unwrap();
    let values = context
        .eval("var probeSym = Symbol(); var probeTarget = {set [probeSym](v) {}}; [probeTarget, probeSym]")
        .unwrap();
    if let crate::Value::Object(array) = values {
        let target = match &array.borrow().elements[0] {
            crate::Value::Object(target) => target.clone(),
            _ => panic!(),
        };
        let symbol = match &array.borrow().elements[1] {
            crate::Value::Symbol(symbol) => symbol.clone(),
            _ => panic!(),
        };
        assert!(target.borrow().setters.contains_key(&symbol.property_key()));
    }
    assert_eq!(
        eval("var descriptorSym = Symbol(); typeof Object.getOwnPropertyDescriptor({set [descriptorSym](v) {},}, descriptorSym).set").unwrap(),
        crate::Value::String("function".into())
    );
    assert_eq!(
        eval("var direct = 1; var directSym = Symbol(); var directTarget = {set [directSym](v) { direct = v; }}; directTarget[directSym] = 2; direct").unwrap(),
        crate::Value::Number(2.0)
    );
    assert_eq!(
        eval("var frozenDirect = 1; var frozenSym = Symbol(); var frozenTarget = {set [frozenSym](v) { frozenDirect = v; }}; Object.freeze(frozenTarget); frozenTarget[frozenSym] = 2; frozenDirect").unwrap(),
        crate::Value::Number(2.0)
    );
    assert_eq!(
        eval("var value = 1; var sym = Symbol(); var target = {set [sym](v) { value = v; }}; Object.freeze(target); Object.assign(target, {[sym]: 2}); value").unwrap(),
        crate::Value::Number(2.0)
    );
}

#[test]
fn object_assign_copies_symbol_property() {
    assert_eq!(
        eval("var inspectSym = Symbol(); var inspectSource = {[inspectSym]: 2}; Object.getOwnPropertySymbols(inspectSource).length").unwrap(),
        crate::Value::Number(1.0)
    );
    assert_eq!(
        eval("var sym = Symbol(); var source = {[sym]: 2}; Object.assign({}, source)[sym]")
            .unwrap(),
        crate::Value::Number(2.0)
    );
}

#[test]
fn object_assign_throws_for_frozen_data_property() {
    assert!(eval("var target = Object.freeze({x: 1}); Object.assign(target, {x: 2})").is_err());
}

#[test]
fn object_assign_throws_when_target_cannot_create_property() {
    assert!(
        eval("var target = Object.preventExtensions({}); Object.assign(target, {x: 1})").is_err()
    );
}

#[test]
fn object_create_reads_property_descriptors_with_receiver() {
    assert_eq!(
        eval("var result = false; Object.defineProperty(Math, 'prop', {get: function() { result = this === Math; return {}; }, enumerable: true}); Object.create({}, Math); result").unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_create_honors_inherited_configurable_descriptor() {
    assert_eq!(
        eval("var proto = {}; Object.defineProperty(proto, 'configurable', {get: function() { return true; }}); var C = function() {}; C.prototype = proto; var d = new C(); var o = Object.create({}, {x: d}); delete o.x; o.hasOwnProperty('x')").unwrap(),
        crate::Value::Boolean(false)
    );
}

#[test]
fn strict_object_create_descriptor_getter_receives_arguments_receiver() {
    assert_eq!(
        eval("'use strict'; var result = false; var args = (function() { return arguments; })(); Object.defineProperty(args, 'prop', {get: function() { result = this === args; return {}; }, enumerable: true}); Object.create({}, args); result").unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn strict_object_to_string_call_preserves_arguments_receiver() {
    assert_eq!(
        eval("'use strict'; var args = (function() { return arguments; })(); Object.prototype.toString.call(args)").unwrap(),
        crate::Value::String("[object Arguments]".into())
    );
}

#[test]
fn function_accessor_defined_by_object_define_property_is_readable() {
    assert_eq!(
        eval("var f = function() {}; Object.defineProperty(f, 'value', {get: function() { return 'x'; }}); f.value").unwrap(),
        crate::Value::String("x".into())
    );
}

#[test]
fn date_accessor_defined_by_object_define_property_is_readable() {
    assert_eq!(
        eval("var d = new Date(0); Object.defineProperty(d, 'prop', {get: function() { return {}; }}); typeof d.prop").unwrap(),
        crate::Value::String("object".into())
    );
}

#[test]
fn constructed_object_does_not_enumerate_inherited_properties() {
    assert_eq!(
        eval("var proto = {prop: {}}; var C = function() {}; C.prototype = proto; var child = new C(); Object.keys(child).length").unwrap(),
        crate::Value::Number(0.0)
    );
}

#[test]
fn object_create_rejects_setter_only_property_descriptor() {
    assert!(eval(
        "var props = {}; Object.defineProperty(props, 'prop', {set: function() {}, enumerable: true}); Object.create({}, props)"
    )
    .is_err());
}

#[test]
fn object_create_allows_setter_only_set_descriptor_field() {
    assert_eq!(
        eval("var d = {}; Object.defineProperty(d, 'set', {set: function() {}}); var o = Object.create({}, {prop: d}); typeof Object.getOwnPropertyDescriptor(o, 'prop').set")
            .unwrap(),
        crate::Value::String("undefined".into())
    );
}

#[test]
fn object_create_reads_properties_from_boxed_string() {
    assert_eq!(
        eval("var props = new String(); props.prop = {value: 12, enumerable: true}; Object.create({}, props).prop")
            .unwrap(),
        crate::Value::Number(12.0)
    );
}

#[test]
fn object_create_prefers_own_accessor_over_inherited_descriptor() {
    assert_eq!(
        eval("var p = {prop: {value: 12}}; var C = function() {}; C.prototype = p; var child = new C(); Object.defineProperty(child, 'prop', {get: function() { return {value: 9}; }, enumerable: true}); Object.create({}, child).prop")
            .unwrap(),
        crate::Value::Number(9.0)
    );
}

#[test]
fn object_create_rejects_undefined_property_descriptor() {
    assert!(eval("Object.create({}, {prop: undefined})").is_err());
}

#[test]
fn object_create_reads_inherited_enumerable_descriptor_field() {
    assert_eq!(
        eval("var p = {}; Object.defineProperty(p, 'enumerable', {get: function() { return true; }}); var C = function() {}; C.prototype = p; var d = new C(); var o = Object.create({}, {prop: d}); Object.keys(o).join(',')")
            .unwrap(),
        crate::Value::String("prop".into())
    );
}

#[test]
fn object_create_calls_boolean_source_getter_with_boolean_receiver() {
    assert_eq!(
        eval("'use strict'; var props = new Boolean(true); var result = false; Object.defineProperty(props, 'prop', {get: function() { result = this instanceof Boolean; return {}; }, enumerable: true}); Object.create({}, props); result")
            .unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_create_boxes_bigint_properties_argument() {
    assert!(eval("Object.create({}, 1n)").is_ok());
}

#[test]
fn object_create_reads_function_properties_argument() {
    assert_eq!(
        eval("var p = function() {}; p.prop = {value: 12, enumerable: true}; Object.create({}, p).prop")
            .unwrap(),
        crate::Value::Number(12.0)
    );
}

#[test]
fn object_define_properties_has_two_parameters() {
    assert_eq!(
        eval("Object.defineProperties.length").unwrap(),
        crate::Value::Number(2.0)
    );
}

#[test]
fn object_define_properties_rejects_undefined_target() {
    assert!(eval("Object.defineProperties(undefined, {})").is_err());
}

#[test]
fn object_define_properties_rejects_null_properties() {
    assert!(eval("Object.defineProperties({}, null)").is_err());
}

#[test]
fn object_define_properties_calls_array_getter_with_array_receiver() {
    assert_eq!(
        eval("var obj = {}; var props = []; var result = false; Object.defineProperty(props, 'prop', {get: function() { result = this instanceof Array; return {}; }, enumerable: true}); Object.defineProperties(obj, props); result")
            .unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_define_properties_calls_boolean_getter_with_boolean_receiver() {
    assert_eq!(
        eval("'use strict'; var obj = {}; var props = new Boolean(true); var result = false; Object.defineProperty(props, 'prop', {get: function() { result = this instanceof Boolean; return {}; }, enumerable: true}); Object.defineProperties(obj, props); result")
            .unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_define_properties_reads_function_properties_source() {
    assert_eq!(
        eval("var obj = {}; var props = function() {}; props.prop = {value: 7, enumerable: true}; Object.defineProperties(obj, props); obj.prop")
            .unwrap(),
        crate::Value::Number(7.0)
    );
}

#[test]
fn object_define_properties_reads_function_descriptor_value() {
    assert_eq!(
        eval("var obj = {}; var func = function(a, b) { return a + b; }; func.value = 'Function'; Object.defineProperties(obj, {property: func}); obj.property")
            .unwrap(),
        crate::Value::String("Function".into())
    );
}

#[test]
fn object_define_properties_rejects_null_descriptor() {
    assert!(eval("Object.defineProperties({}, {prop: null})").is_err());
}

#[test]
fn object_define_properties_calls_date_getter_with_date_receiver() {
    assert_eq!(
        eval("var obj = {}; var props = new Date(0); var result = false; Object.defineProperty(props, 'prop', {get: function() { result = this instanceof Date; return {}; }, enumerable: true}); Object.defineProperties(obj, props); result")
            .unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_define_properties_skips_regexp_internal_properties() {
    assert_eq!(
        eval("var obj = {}; var props = new RegExp(); var result = false; Object.defineProperty(props, 'prop', {get: function() { result = this instanceof RegExp; return {}; }, enumerable: true}); Object.defineProperties(obj, props); result")
            .unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_define_properties_calls_string_getter_with_string_receiver() {
    assert_eq!(
        eval("var obj = {}; var props = new String(); var result = false; Object.defineProperty(props, 'prop', {get: function() { result = this instanceof String; return {}; }, enumerable: true}); Object.defineProperties(obj, props); result")
            .unwrap(),
        crate::Value::Boolean(true)
    );
}

#[test]
fn object_define_properties_undefined_get_preserves_existing_setter() {
    assert_eq!(
        eval("var obj = {}; Object.defineProperty(obj, 'foo', {get: function() { return 1; }, set: function(v) { obj.x = v; }, enumerable: true, configurable: true}); Object.defineProperties(obj, {foo: {get: undefined}}); obj.foo = 7; obj.x")
            .unwrap(),
        crate::Value::Number(7.0)
    );
}

#[test]
fn object_define_properties_empty_accessor_descriptor_preserves_accessors() {
    assert_eq!(
        eval("var obj = {}; Object.defineProperty(obj, 'foo', {get: function() { return 10; }, set: function(v) { obj.x = v; }, enumerable: true, configurable: true}); Object.defineProperties(obj, {foo: {enumerable: false}}); obj.foo = 7; obj.foo + obj.x")
            .unwrap(),
        crate::Value::Number(17.0)
    );
}

#[test]
fn object_define_properties_undefined_set_removes_existing_setter() {
    assert_eq!(
        eval("var obj = {}; Object.defineProperty(obj, 'foo', {get: function() { return 10; }, set: function(v) { obj.x = v; }, enumerable: true, configurable: true}); Object.defineProperties(obj, {foo: {set: undefined}}); typeof Object.getOwnPropertyDescriptor(obj, 'foo').set")
            .unwrap(),
        crate::Value::String("undefined".into())
    );
}

#[test]
fn object_define_properties_new_get_preserves_existing_setter() {
    assert_eq!(
        eval("var obj = {}; Object.defineProperty(obj, 'foo', {get: function() { return 1; }, set: function(v) { obj.x = v; }, enumerable: true, configurable: true}); Object.defineProperties(obj, {foo: {get: function() { return 2; }}}); obj.foo = 7; obj.foo + obj.x")
            .unwrap(),
        crate::Value::Number(9.0)
    );
}

#[test]
fn object_define_properties_rejects_shrinking_past_nonconfigurable_array_element() {
    assert!(eval("var a = [0, 1]; Object.defineProperty(a, '1', {value: 1, configurable: false}); Object.defineProperties(a, {length: {value: 1}})").is_err());
}

#[test]
fn array_length_descriptor_reports_actual_length_after_rejected_shrink() {
    assert_eq!(
        eval("var a = [0, 1]; Object.defineProperty(a, '1', {value: 1, configurable: false}); try { Object.defineProperties(a, {length: {value: 1}}); } catch (e) {} Object.getOwnPropertyDescriptor(a, 'length').value")
            .unwrap(),
        crate::Value::Number(2.0)
    );
}

#[test]
fn object_define_properties_updates_array_length_flags() {
    assert_eq!(
        eval("var a = []; Object.defineProperties(a, {length: {writable: true, enumerable: false, configurable: false}}); a.length = 2; Object.getOwnPropertyDescriptor(a, 'length').value")
            .unwrap(),
        crate::Value::Number(2.0)
    );
}

#[test]
fn object_define_properties_rejects_enumerable_array_length() {
    assert!(eval("var a = []; Object.defineProperties(a, {length: {enumerable: true}})").is_err());
}

#[test]
fn object_define_properties_rejects_accessor_array_length() {
    assert!(eval(
        "var a = []; Object.defineProperties(a, {length: {get: function() { return 2; }}})"
    )
    .is_err());
}

#[test]
fn object_define_properties_rejects_undefined_array_length_with_range_error() {
    assert!(eval("Object.defineProperties([], {length: {value: undefined}})").is_err());
}

#[test]
fn object_define_properties_coerces_null_array_length_to_zero() {
    assert_eq!(
        eval("var a = [0, 1]; Object.defineProperties(a, {length: {value: null}}); a.length")
            .unwrap(),
        crate::Value::Number(0.0)
    );
}

#[test]
fn object_define_properties_coerces_true_array_length_to_one() {
    assert_eq!(
        eval("var a = []; Object.defineProperties(a, {length: {value: true}}); a.length").unwrap(),
        crate::Value::Number(1.0)
    );
}

#[test]
fn object_define_properties_normalizes_negative_zero_array_length() {
    assert_eq!(
        eval("var a = [0, 1]; Object.defineProperties(a, {length: {value: -0}}); a.length")
            .unwrap(),
        crate::Value::Number(0.0)
    );
}

#[test]
fn object_define_properties_rejects_nonprimitive_array_length_conversion() {
    assert!(eval("Object.defineProperties([], {length: {value: {toString: function() { return {}; }, valueOf: function() { return {}; }}}})").is_err());
}

#[test]
fn object_define_properties_rejects_array_length_above_uint32() {
    assert!(eval("Object.defineProperties([], {length: {value: 4294967296}})").is_err());
}

#[test]
fn object_define_properties_rejects_index_beyond_nonwritable_length() {
    assert!(eval("var a = [1, 2, 3]; Object.defineProperty(a, 'length', {writable: false}); Object.defineProperties(a, {'3': {value: 'abc'}})").is_err());
}

#[test]
fn object_define_properties_absent_value_does_not_copy_inherited_value() {
    assert_eq!(
        eval("Object.defineProperty(Array.prototype, '0', {value: 11, configurable: true}); var a = []; Object.defineProperties(a, {'0': {configurable: false}}); typeof a[0]")
            .unwrap(),
        crate::Value::String("undefined".into())
    );
}

#[test]
fn object_define_properties_failed_array_shrink_preserves_nonconfigurable_value() {
    assert_eq!(
        eval("var a = [0, 1, 2]; Object.defineProperty(a, '1', {configurable: false}); Object.defineProperty(a, '2', {configurable: true}); try { Object.defineProperties(a, {length: {value: 1}}); } catch (e) {} a.length + ':' + a[1] + ':' + a.hasOwnProperty('2')")
            .unwrap(),
        crate::Value::String("2:1:false".into())
    );
}

#[test]
fn object_define_properties_rejects_new_index_on_nonextensible_array() {
    assert!(eval(
        "var a = []; Object.preventExtensions(a); Object.defineProperties(a, {'0': {value: 1}})"
    )
    .is_err());
}

#[test]
fn object_define_properties_empty_existing_array_descriptor_preserves_attributes() {
    assert_eq!(
        eval("var a = []; a[0] = 101; Object.defineProperties(a, {'0': {}}); var d = Object.getOwnPropertyDescriptor(a, '0'); a[0] = 202; [a[0], d.writable, d.enumerable, d.configurable].join(':')").unwrap(),
        crate::Value::String("202:true:true:true".into())
    );
}

#[test]
fn object_define_properties_allows_same_nan_on_nonconfigurable_array_property() {
    assert!(eval("var a = []; Object.defineProperty(a, '0', {value: NaN}); Object.defineProperties(a, {'0': {value: NaN}})").is_ok());
}

#[test]
fn object_define_properties_rejects_enumerable_change_on_nonconfigurable_index() {
    assert!(eval("var a = []; Object.defineProperty(a, '1', {value: 3, configurable: false, enumerable: false}); Object.defineProperties(a, {'1': {value: 13, enumerable: true}})").is_err());
}

#[test]
fn object_define_properties_rejects_accessor_for_nonconfigurable_data_index() {
    assert!(eval("var a = []; Object.defineProperty(a, '1', {value: 3, configurable: false}); Object.defineProperties(a, {'1': {set: function() {}}})").is_err());
}

#[test]
fn object_define_properties_converts_configurable_accessor_to_array_data_property() {
    assert_eq!(
        eval("var a = []; Object.defineProperty(a, '1', {get: function() { return 3; }, configurable: true}); Object.defineProperties(a, {'1': {value: 12}}); var d = Object.getOwnPropertyDescriptor(a, '1'); [a[1], d.value, d.writable, d.enumerable, d.configurable].join(':')").unwrap(),
        crate::Value::String("12:12:false:false:true".into())
    );
}

#[test]
fn object_define_properties_rejects_replacing_nonconfigurable_setter() {
    assert!(eval("var a = []; function set_a(v) { a.marker = v; } Object.defineProperty(a, '1', {set: set_a}); Object.defineProperties(a, {'1': {set: function(v) { a.other = v; }}})").is_err());
}

#[test]
fn object_define_properties_rejects_replacing_nonconfigurable_getter() {
    assert!(eval("var a = []; function get_a() { return 36; } Object.defineProperty(a, '1', {get: get_a}); Object.defineProperties(a, {'1': {get: function() { return 12; }}})").is_err());
}

#[test]
fn object_define_properties_rejects_enumerable_change_on_nonconfigurable_accessor() {
    assert!(eval("var a = {}; Object.defineProperty(a, 'property', {set: function(v) { a.marker = v; }}); Object.defineProperties(a, {property: {enumerable: true}})").is_err());
}

#[test]
fn object_define_properties_redefines_deleted_arguments_mapping() {
    assert_eq!(
        eval("var arg; (function(a, b, c) { arg = arguments; }(0, 1, 2)); delete arg[0]; Object.defineProperties(arg, {'0': {value: 10, writable: true, enumerable: true, configurable: true}}); var d = Object.getOwnPropertyDescriptor(arg, '0'); [arg[0], d.writable, d.enumerable, d.configurable].join(':')").unwrap(),
        crate::Value::String("10:true:true:true".into())
    );
}

#[test]
fn object_define_properties_redefines_deleted_arguments_accessor() {
    assert_eq!(
        eval("var arg; (function(a, b, c) { arg = arguments; }(0, 1, 2)); delete arg[0]; function get_v() { return 10; } function set_v(v) { arg.marker = v; } Object.defineProperties(arg, {'0': {get: get_v, set: set_v, enumerable: true, configurable: true}}); arg[0] = 20; [arg[0], arg.marker, Object.getOwnPropertyDescriptor(arg, '0').enumerable].join(':')").unwrap(),
        crate::Value::String("10:20:true".into())
    );
}

#[test]
fn object_define_properties_arguments_getter_only_has_no_setter() {
    assert_eq!(
        eval("var arg; (function(a, b, c) { arg = arguments; }(0, 1, 2)); function get_a() { return 10; } Object.defineProperty(arg, '0', {get: get_a, enumerable: true, configurable: true}); function get_b() { return 20; } Object.defineProperties(arg, {'0': {get: get_b, enumerable: false, configurable: false}}); var d = Object.getOwnPropertyDescriptor(arg, '0'); [d.get === get_b, typeof d.set, d.configurable, d.enumerable].join(':')").unwrap(),
        crate::Value::String("true:undefined:false:false".into())
    );
}

#[test]
fn object_define_properties_rejects_bigint_descriptor() {
    assert!(eval("Object.defineProperties({}, {a: 0n})").is_err());
}

#[test]
fn object_define_properties_proxy_missing_descriptor_does_not_panic() {
    assert!(eval("var target = {}; var sym = Symbol(); target[sym] = 1; target.foo = 2; target[0] = 3; var keys = []; var proxy = new Proxy(target, {getOwnPropertyDescriptor: function(t, k) { keys.push(k); }}); Object.defineProperties({}, proxy); keys.join(',')").is_ok());
}

#[test]
fn object_define_properties_large_array_index_updates_length() {
    assert_eq!(
        eval("var a = []; Object.defineProperties(a, {'4294967294': {value: 100}}); a.length")
            .unwrap(),
        crate::Value::Number(4294967295.0)
    );
}

#[test]
fn object_define_properties_truncates_array_on_length_shrink() {
    assert_eq!(
        eval("var a = [0, 1]; Object.defineProperties(a, {length: {value: 1}}); a.hasOwnProperty('1')")
            .unwrap(),
        crate::Value::Boolean(false)
    );
}

#[test]
fn array_length_extension_creates_holes() {
    assert_eq!(
        eval("var a = [0, 1]; Object.defineProperties(a, {length: {value: 1}}); a.length = 10; a.hasOwnProperty('1')")
            .unwrap(),
        crate::Value::Boolean(false)
    );
}

#[test]
fn rejected_array_shrink_can_make_length_nonwritable() {
    assert_eq!(
        eval("var a = [0, 1, 2]; Object.defineProperty(a, '1', {configurable: false}); try { Object.defineProperties(a, {length: {value: 0, writable: false}}); } catch (e) {} Object.getOwnPropertyDescriptor(a, 'length').writable")
            .unwrap(),
        crate::Value::Boolean(false)
    );
}

#[test]
fn object_define_properties_rejects_nonconfigurable_function_property() {
    assert!(eval("var f = function() {}; Object.defineProperty(f, 'prop', {value: 11, configurable: false}); Object.defineProperties(f, {prop: {value: 12, configurable: true}})").is_err());
}
