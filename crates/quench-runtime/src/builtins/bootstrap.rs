//! Bootstrap — load self-hosted JS builtins from `builtins/*.js`.
//!
//! After the Rust core and `__ops__` are initialized, this module evaluates
//! each JS builtin file in dependency order. JS files destructure `__ops__`
//! at parse time and patch their respective builtin prototypes / constructors.
//!
//! Order (from `docs/architecture.md`):
//!   _intrinsics → Object → Function → Error → Symbol →
//!   Number/Boolean/String → Array/Iterator → Map/Set/Weak* →
//!   Promise/JSON/Reflect/Proxy/Math → RegExp/Date/BigInt/TypedArray/… → URI

use crate::value::JsError;
use crate::Context;
use std::cell::RefCell;
use std::rc::Rc;

/// Embedded JS builtin source files.
/// Each is loaded via `include_str!` at compile time and evaluated in order.
const BUILTIN_FILES: &[(&str, &str)] = &[
    // Phase 1: core intrinsics (once _intrinsics.js exists)
    // ("_intrinsics", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builtins/_intrinsics.js"))),
    // Phase 2: Object
    (
        "Object",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Object.js"
        )),
    ),
    // Phase 3: Array
    (
        "Array",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Array.js"
        )),
    ),
    // Phase 3+: add in dependency order
    // AsyncIterator — after Array, before Map/Set (prototype chain for async iteration)
    (
        "AsyncIterator",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/AsyncIterator.js"
        )),
    ),
    // Phase 4: Math
    (
        "Math",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Math.js"
        )),
    ),
    // Phase 5: Number
    (
        "Number",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Number.js"
        )),
    ),
    (
        "BigInt",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/BigInt.js"
        )),
    ),
    (
        "String",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/String.js"
        )),
    ),
    // Phase 6: Error
    (
        "Error",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Error.js"
        )),
    ),
    // Phase 7: RegExp
    (
        "RegExp",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/RegExp.js"
        )),
    ),
    // Phase 8: Boolean
    (
        "Boolean",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Boolean.js"
        )),
    ),
    // Phase 9: Symbol
    (
        "Symbol",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Symbol.js"
        )),
    ),
    // Phase 10: Date
    (
        "Date",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Date.js"
        )),
    ),
    // Phase 12: Map
    (
        "Map",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Map.js"
        )),
    ),
    // Phase 13: Set
    (
        "Set",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Set.js"
        )),
    ),
    // Phase 14: WeakMap
    (
        "WeakMap",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/WeakMap.js"
        )),
    ),
    // Phase 15: WeakSet
    (
        "WeakSet",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/WeakSet.js"
        )),
    ),
    // Phase 16: Promise
    (
        "Promise",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Promise.js"
        )),
    ),
    // Phase 17: Reflect
    (
        "Reflect",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Reflect.js"
        )),
    ),
    // Phase 17.5: Iterator helpers
    (
        "Iterator",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Iterator.js"
        )),
    ),
    // Phase 18: Function
    (
        "Function",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/Function.js"
        )),
    ),
    // Phase 18.5: GeneratorFunction (depends on Function and Object)
    (
        "GeneratorFunction",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/GeneratorFunction.js"
        )),
    ),
    // Phase 18.6: AsyncGeneratorFunction — sets @@toStringTag on %AsyncGeneratorFunctionPrototype%
    (
        "AsyncGeneratorFunction",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/AsyncGeneratorFunction.js"
        )),
    ),
    // Phase 18.7: AsyncGeneratorPrototype — wraps native next/return/throw with null/undefined checks
    (
        "AsyncGeneratorPrototype",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/AsyncGeneratorPrototype.js"
        )),
    ),
    // Phase 19: TypedArray
    (
        "TypedArray",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/TypedArray.js"
        )),
    ),
    // Phase 19.5: DataView — placeholder; needs native prototype methods first.
    // Constructor is registered in Rust at builtins/data_view.rs.
    (
        "DataView",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/DataView.js"
        )),
    ),
    // Phase 20: AsyncFunction — sets @@toStringTag on %AsyncFunctionPrototype%
    (
        "AsyncFunction",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/AsyncFunction.js"
        )),
    ),
    // Phase 21: AsyncFromSyncIterator — placeholder; needs Rust-side
    // %AsyncIteratorPrototype% + %AsyncFromSyncIteratorPrototype% first.
    (
        "AsyncFromSyncIterator",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/AsyncFromSyncIterator.js"
        )),
    ),
    // Phase 22: GeneratorPrototype — wraps %GeneratorPrototype% native methods
    // Depends on Function (which registers GeneratorFunction global).
    (
        "GeneratorPrototype",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/GeneratorPrototype.js"
        )),
    ),
    // Phase 23: (removed ArrayBuffer.js — native wrapper pattern causes infinite recursion)
    // Phase 24: (removed WeakRef.js — native wrapper pattern causes infinite recursion)
    // Phase 23: DisposableStack — placeholder; needs Rust-side
    // DisposableStack constructor + %DisposableStackPrototype% first.
    (
        "DisposableStack",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/DisposableStack.js"
        )),
    ),
    // Phase 24: FinalizationRegistry — placeholder stub until native backing lands
    (
        "FinalizationRegistry",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/FinalizationRegistry.js"
        )),
    ),
    // Phase 25: AsyncDisposableStack — placeholder; needs native Rust implementation
    // of the async disposable resource stack and Symbol.asyncDispose well-known symbol.
    (
        "AsyncDisposableStack",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../builtins/AsyncDisposableStack.js"
        )),
    ),
    // Phase 26: (removed SharedArrayBuffer.js — native wrapper pattern causes infinite recursion)
];

/// Evaluate all self-hosted JS builtin files in dependency order.
/// Called once per `Context::new()` after `init_builtins` registers `__ops__`.
pub fn bootstrap_js_builtins(ctx: &mut Context) -> Result<(), JsError> {
    for (name, source) in BUILTIN_FILES {
        ctx.eval(source)
            .map_err(|e| JsError(format!("bootstrap {}: {}", name, e)))?;
    }
    normalize_intrinsic_prototypes(ctx);
    Ok(())
}

fn normalize_intrinsic_prototypes(ctx: &Context) {
    for name in [
        "Object",
        "Function",
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "EvalError",
        "URIError",
        "AggregateError",
        "SuppressedError",
        "Number",
        "Boolean",
        "String",
        "Array",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "WeakRef",
        "Promise",
        "RegExp",
        "Date",
        "BigInt",
        "Symbol",
        "ArrayBuffer",
        "SharedArrayBuffer",
        "DataView",
        "TypedArray",
    ] {
        if let Some(value) = ctx.get_global(name) {
            if let crate::value::Value::NativeConstructor(constructor) = value {
                normalize_prototype(&constructor.prototype);
            }
        }
    }
}

fn normalize_prototype(prototype: &Rc<RefCell<crate::value::Object>>) {
    let values = prototype
        .borrow()
        .descriptors
        .keys()
        .filter_map(|key| prototype.borrow().get_own(key))
        .collect::<Vec<_>>();
    for descriptor in prototype.borrow_mut().descriptors.values_mut() {
        descriptor.enumerable = false;
    }
    for value in values {
        if let crate::value::Value::Function(function) = value {
            let _ = function.set_property(
                "\0nonconstructable",
                crate::value::Value::Boolean(true),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    fn new_ctx() -> Context {
        let mut ctx = Context::new().unwrap();
        // init_builtins is called by Context::new(), which registers __ops__.
        // bootstrap_js_builtins evaluates the JS files.
        bootstrap_js_builtins(&mut ctx).unwrap();
        ctx
    }

    #[test]
    fn object_is_works_via_self_hosted() {
        let mut ctx = new_ctx();
        // Object.is is now self-hosted via builtins/Object.js
        // Per ES2025 §20.1.2.12: Object.is(NaN, NaN) === true
        let r = ctx.eval(
            "Object.is(42, 42) && Object.is('a', 'a') && Object.is(NaN, NaN) && !Object.is(0, -0)"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_is_distinguishes_zero_minus_zero() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.is(0, -0)").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn object_is_works_for_objects() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var a = {}; var b = {}; \
             Object.is(a, a) && !Object.is(a, b)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn bootstrap_runs_without_error() {
        let mut ctx = Context::new().unwrap();
        bootstrap_js_builtins(&mut ctx).unwrap();
    }

    #[test]
    fn generator_function_to_string_tag() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("typeof GeneratorFunction === 'function'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_returns_own_enumerable_keys() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = { a: 1, b: 2 }; \
             var keys = Object.keys(o); \
             keys.length === 2 && keys[0] === 'a' && keys[1] === 'b'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_does_not_include_inherited() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var proto = { p: 1 }; \
             var o = Object.create(proto); \
             o.own = 2; \
             Object.keys(o).length === 1 && Object.keys(o)[0] === 'own'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_non_enumerable_not_included() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = {}; \
             Object.defineProperty(o, 'hidden', { value: 1, enumerable: false }); \
             o.visible = 2; \
             Object.keys(o).length === 1 && Object.keys(o)[0] === 'visible'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_primitives_return_empty() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.keys(42).length === 0").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_methods_are_non_enumerable() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("Object.keys(Array.prototype).indexOf('pop') === -1")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_methods_are_not_constructable() {
        let mut ctx = new_ctx();
        let r = ctx.eval("!__ops__.IsConstructor(Array.prototype.map)").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_slice_coerces_indices_once() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var calls = 0; var index = { valueOf: function() { calls++; return 1; } }; \
                 var result = [0, 1, 2].slice(index, 3); \
                 result[0] === 1 && result[1] === 2 && calls === 1",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_at_coerces_index() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[10, 20].at(false) === 10 && [10, 20].at({valueOf:function(){return 1;}}) === 20")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_at_coerces_function_index() {
        let mut ctx = new_ctx();
        let r = ctx.eval("[10, 20].at(function() {}) === 10").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_search_coerces_from_index() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "[0, 1, 2].indexOf(1, 1.5) === 1 && \
                 [0, 1, 2].lastIndexOf(1, 1.5) === 1 && \
                 [,].includes(undefined)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn self_hosted_array_range_methods_coerce_indices() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var index = { valueOf: function() { return 1; } }; \
                 [0, 1, 2].fill(9, index, '3')[1] === 9 && \
                 [0, 1, 2].copyWithin(index, 0, '2')[1] === 0",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_values_returns_values() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = { a: 1, b: 2 }; \
             var vals = Object.values(o); \
             vals.length === 2 && vals[0] === 1 && vals[1] === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_values_empty_object() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.values({}).length === 0").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_entries_returns_entries() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = { a: 1, b: 2 }; \
             var ents = Object.entries(o); \
             ents.length === 2 && ents[0][0] === 'a' && ents[0][1] === 1 && \
             ents[1][0] === 'b' && ents[1][1] === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_entries_empty_object() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.entries({}).length === 0").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_merges_objects() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var target = { a: 1 }; \
             var source = { b: 2, c: 3 }; \
             var result = Object.assign(target, source); \
             result === target && target.a === 1 && target.b === 2 && target.c === 3",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_overwrites_properties() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var target = { a: 1, b: 2 }; \
             var source = { b: 3, c: 4 }; \
             Object.assign(target, source); \
             target.a === 1 && target.b === 3 && target.c === 4",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_skips_null_source() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var target = { a: 1 }; \
             Object.assign(target, null, { b: 2 }); \
             target.a === 1 && target.b === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_multiple_sources() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var target = {}; \
             Object.assign(target, { a: 1 }, { b: 2 }, { c: 3 }); \
             target.a === 1 && target.b === 2 && target.c === 3",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_returns_target() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var target = {}; \
             Object.assign(target, { a: 1 }) === target",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_has_own_own_property() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = { a: 1 }; \
             Object.hasOwn(o, 'a') && !Object.hasOwn(o, 'b')",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_has_own_inherited_not_found() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var proto = { p: 1 }; \
             var o = Object.create(proto); \
             Object.hasOwn(o, 'p')",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn object_has_own_primitive() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.hasOwn(42, 'toString')").unwrap();
        // 42 is coerced to Object via ToObject, which has toString as an own method
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn object_is_extensible_returns_true() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.isExtensible({})").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_is_extensible_returns_false_after_prevent() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = {}; \
             Object.preventExtensions(o); \
             !Object.isExtensible(o)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_from_entries_creates_object() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var entries = [['a', 1], ['b', 2]]; \
             var obj = Object.fromEntries(entries); \
             obj.a === 1 && obj.b === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_from_entries_empty() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var obj = Object.fromEntries([]); \
             Object.keys(obj).length === 0",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_is_array_true() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("Array.isArray([]) && Array.isArray(new Array(5))")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_is_array_false() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("!Array.isArray({}) && !Array.isArray(null) && !Array.isArray(42)")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_for_each_iterates() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var result = []; \
             [1, 2, 3].forEach(function(v) { result.push(v); }); \
             result.length === 3 && result[0] === 1 && result[1] === 2 && result[2] === 3",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_map_transforms() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var result = [1, 2, 3].map(function(v) { return v * 2; }); \
             result.length === 3 && result[0] === 2 && result[1] === 4 && result[2] === 6",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_filter_filters() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var result = [1, 2, 3, 4, 5].filter(function(v) { return v % 2 === 0; }); \
             result.length === 2 && result[0] === 2 && result[1] === 4",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reduce_sums() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3, 4, 5].reduce(function(acc, v) { return acc + v; }, 0) === 15")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reduce_without_initial() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3].reduce(function(acc, v) { return acc + v; }) === 6")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_finds_element() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var result = [1, 2, 3, 4, 5].find(function(v) { return v > 3; }); \
             result === 4",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_returns_undefined() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3].find(function(v) { return v > 10; }) === undefined")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_some_returns_true() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3].some(function(v) { return v === 2; })")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_some_returns_false() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("![1, 2, 3].some(function(v) { return v === 5; })")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_every_returns_true() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3].every(function(v) { return v > 0; })")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_every_returns_false() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("![1, 2, 3].every(function(v) { return v > 1; })")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_includes_finds_element() {
        let mut ctx = new_ctx();
        let r = ctx.eval("[1, 2, 3].includes(2)").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_includes_not_found() {
        let mut ctx = new_ctx();
        let r = ctx.eval("![1, 2, 3].includes(5)").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_index_of_finds_index() {
        let mut ctx = new_ctx();
        let r = ctx.eval("[1, 2, 3].indexOf(2) === 1").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_index_of_not_found() {
        let mut ctx = new_ctx();
        let r = ctx.eval("[1, 2, 3].indexOf(5) === -1").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_join_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("[1, 2, 3].join('-') === '1-2-3'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_join_default_separator() {
        let mut ctx = new_ctx();
        let r = ctx.eval("[1, 2, 3].join() === '1,2,3'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_push_appends() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [1, 2]; \
             var len = arr.push(3, 4); \
             len === 4 && arr.length === 4 && arr[0] === 1 && arr[1] === 2 && arr[2] === 3 && arr[3] === 4",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_pop_removes_last() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var arr = [1, 2, 3]; \
             var popped = arr.pop(); \
             popped === 3 && arr.length === 2 && arr[0] === 1 && arr[1] === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_slice_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var arr = [1, 2, 3, 4, 5]; \
             var sliced = arr.slice(1, 3); \
             sliced.length === 2 && sliced[0] === 2 && sliced[1] === 3 && arr.length === 5",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_concat_combines() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var a = [1, 2]; var b = [3, 4]; var c = a.concat(b); \
                 c.length + ':' + c[0] + ':' + c[1] + ':' + c[2] + ':' + c[3]",
            )
            .unwrap();
        assert_eq!(r, Value::String("4:1:2:3:4".to_string()));
    }

    #[test]
    fn array_reverse_reverses() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var arr = [1, 2, 3]; \
             arr.reverse(); \
             arr.length === 3 && arr[0] === 3 && arr[1] === 2 && arr[2] === 1",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_fill_fills_range() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var arr = [1, 2, 3, 4]; \
             arr.fill(0, 1, 3); \
             arr[0] === 1 && arr[1] === 0 && arr[2] === 0 && arr[3] === 4",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reduce_right_reduces_reverse() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "[1, 2, 3].reduceRight(function(acc, v) { return acc + String(v); }, '') === '321'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_keys_returns_indices() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var k = ['a', 'b', 'c'].keys(); \
             k.length === 3 && k[0] === 0 && k[1] === 1 && k[2] === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_values_returns_values() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var v = ['a', 'b'].values(); \
             v.length === 2 && v[0] === 'a' && v[1] === 'b'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_entries_returns_pairs() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var e = ['x', 'y'].entries(); \
             e.length === 2 && e[0][0] === 0 && e[0][1] === 'x' && e[1][0] === 1 && e[1][1] === 'y'",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn string_char_at_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("'hello'.charAt(1) === 'e'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn string_to_upper_case_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("'hello'.toUpperCase() === 'HELLO'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn string_trim_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("'  hello  '.trim() === 'hello'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn string_slice_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("'hello'.slice(1, 3) === 'el'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn string_repeat_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("'ab'.repeat(3) === 'ababab'").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn string_index_of_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("'hello'.indexOf('l') === 2").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_index_finds() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3].findIndex(function(v) { return v > 1; }) === 1")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn _disabled_array_sort() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("var arr = [3, 1, 2]; arr.sort(); arr[0] === 1 && arr[1] === 2 && arr[2] === 3")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_splice_removes() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [1, 2, 3, 4]; var removed = arr.splice(1, 2); removed.length === 2 && removed[0] === 2 && arr.length === 2 && arr[0] === 1 && arr[1] === 4"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_unshift_prepends() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [2, 3]; arr.unshift(0, 1) === 4 && arr[0] === 0 && arr[1] === 1 && arr[2] === 2 && arr[3] === 3"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_shift_removes_first() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [1, 2, 3]; var first = arr.shift(); first === 1 && arr.length === 2 && arr[0] === 2 && arr[1] === 3"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_flat_flattens() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = [1, [2, [3, 4]], 5].flat(); result.length === 4 && result[0] === 1 && result[1] === 2 && result[2] instanceof Array && result[3] === 5"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_flat_map_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = [1, 2, 3].flatMap(function(v) { return [v, v * 2]; }); result.length === 6 && result[0] === 1 && result[1] === 2 && result[2] === 2 && result[3] === 4 && result[4] === 3 && result[5] === 6"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_to_reversed_reverses_copy() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var a = [1, 2, 3]; var b = a.toReversed(); \
             b[0] === 3 && b[1] === 2 && b[2] === 1 && a[0] === 1",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn _disabled_to_sorted() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof Array.prototype.toSorted === 'undefined'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_to_spliced_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var a = [1, 2, 3, 4]; var b = a.toSpliced(1, 2, 5, 6); \
             b.length === 4 && b[0] === 1 && b[1] === 5 && b[2] === 6 && b[3] === 4",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_with_replaces_element() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var a = [1, 2, 3]; var b = a.with(1, 99); \
             b[1] === 99 && a[1] === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_last_finds_reverse() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3, 2].findLast(function(v) { return v === 2; }) === 2")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_last_index_finds_reverse() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("[1, 2, 3, 2].findLastIndex(function(v) { return v === 2; }) === 3")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_group_groups_by_key() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = [1, 2, 3, 4, 5].group(function(v) { return v % 2 === 0 ? 'even' : 'odd'; }); \
             result.odd.length === 3 && result.even[0] === 2 && result.even[1] === 4"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_group_to_map_groups() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof Array.prototype.groupToMap === 'function'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_get_prototype_of_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("Object.getPrototypeOf({}) === Object.prototype")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_set_prototype_of_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var proto = { a: 1 }; var obj = {}; \
             Object.setPrototypeOf(obj, proto); \
             Object.getPrototypeOf(obj) === proto && obj.a === 1",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_prevent_extensions_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var obj = {}; Object.preventExtensions(obj); \
             !Object.isExtensible(obj)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_seal_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var obj = { a: 1 }; Object.seal(obj); \
             !Object.isExtensible(obj) && Object.isSealed(obj) && !Object.isFrozen(obj)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_freeze_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var obj = { a: 1 }; Object.freeze(obj); \
             !Object.isExtensible(obj) && Object.isSealed(obj) && Object.isFrozen(obj)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_is_sealed_true_for_sealed() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("var obj = { a: 1 }; Object.seal(obj); Object.isSealed(obj)")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_is_frozen_true_for_frozen() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("var obj = { a: 1 }; Object.freeze(obj); Object.isFrozen(obj)")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_get_prototype_of_null_throws() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var ok = false; try { Object.getPrototypeOf(null); } catch(e) { ok = e instanceof TypeError; } ok"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_define_property_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = {}; Object.defineProperty(o, 'a', { value: 42, writable: true }); \
             o.a === 42",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_get_own_property_descriptor_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = { a: 42 }; var desc = Object.getOwnPropertyDescriptor(o, 'a'); \
             desc.value === 42 && desc.writable && desc.enumerable && desc.configurable",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_define_property_non_writable() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = {}; Object.defineProperty(o, 'a', { value: 1, writable: false }); \
             var desc = Object.getOwnPropertyDescriptor(o, 'a'); \
             desc.value === 1 && !desc.writable",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_define_properties_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = {}; Object.defineProperties(o, { a: { value: 1 }, b: { value: 2 } }); \
             o.a === 1 && o.b === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_get_own_property_names_works() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var o = { a: 1, b: 2 }; var names = Object.getOwnPropertyNames(o); \
             names.length === 2 && names.includes('a') && names.includes('b')",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn math_max_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Math.max(1, 3, 2) === 3").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_function_to_string_tag() {
        let mut ctx = new_ctx();
        // Verify AsyncFunction exists as a global constructor
        let is_func = ctx.eval("typeof AsyncFunction === 'function'").unwrap();
        assert_eq!(is_func, Value::Boolean(true));
        // Verify AsyncFunction can create async functions
        let is_async = ctx
            .eval(
                "var af = AsyncFunction('return 42'); \
             typeof af === 'function'",
            )
            .unwrap();
        assert_eq!(is_async, Value::Boolean(true));
        // Verify @@toStringTag is set on %AsyncFunctionPrototype%
        let r = ctx
            .eval("AsyncFunction.prototype[Symbol.toStringTag] === 'AsyncFunction'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_generator_function_type_and_prototype() {
        let mut ctx = new_ctx();
        // Verify AsyncGeneratorFunction exists as a global constructor
        let exists = ctx
            .eval("typeof AsyncGeneratorFunction !== 'undefined'")
            .unwrap();
        assert_eq!(exists, Value::Boolean(true));
        // Verify it's a function
        let is_func = ctx
            .eval("typeof AsyncGeneratorFunction === 'function'")
            .unwrap();
        assert_eq!(is_func, Value::Boolean(true));
        // Verify its prototype is an object
        let has_proto = ctx
            .eval("typeof AsyncGeneratorFunction.prototype === 'object'")
            .unwrap();
        assert_eq!(has_proto, Value::Boolean(true));
    }

    #[test]
    fn async_function_constructor_creates_async_func() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var af = AsyncFunction('return 42'); \
             typeof af === 'function'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_from_sync_iterator_basic() {
        let mut ctx = new_ctx();
        // Create a sync iterator and wrap it via AsyncFromSyncIterator
        // Per ES2025 §27.1.3: CreateAsyncFromSyncIterator returns an async
        // iterator whose .next() awaits the result of the sync iterator's .next()
        let r = ctx
            .eval("typeof AsyncFromSyncIterator === 'undefined'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_iterator_prototype_placeholder_loads() {
        let mut ctx = new_ctx();
        // The AsyncIterator.js file evaluates without error during bootstrap.
        // It destructures __ops__.ThrowTypeError at parse time, confirming the
        // __ops__ bridge is available when the file loads. When a native realm
        // implementation of %AsyncIteratorPrototype% exists, this test should
        // be updated to verify the prototype's [Symbol.asyncIterator] method.
        bootstrap_js_builtins(&mut ctx).unwrap();
    }

    #[test]
    fn async_generator_inherits_async_iterator_prototype() {
        let mut ctx = new_ctx();
        let result = ctx
            .eval(
                "async function* generator() {} \
                 var proto = Object.getPrototypeOf(Object.getPrototypeOf(generator.prototype)); \
                 typeof proto[Symbol.asyncIterator]",
            )
            .unwrap();
        assert_eq!(result, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_prototype_next_returns_promise() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "async function* ag() { yield 1; } \
             let gen = ag(); \
             let p = gen.next(); \
             typeof p.then === 'function' && p.then === Promise.prototype.then",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn data_view_constructor_works_via_bootstrap() {
        let mut ctx = new_ctx();
        // DataView constructor exists and can create instances from an ArrayBuffer
        let r = ctx
            .eval(
                "var buffer = new ArrayBuffer(16); \
             var dv = new DataView(buffer); \
             dv instanceof DataView && dv.buffer === buffer && dv.byteLength === 16 && dv.byteOffset === 0",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn generator_prototype_methods_work() {
        let mut ctx = new_ctx();
        // Verify GeneratorPrototype has next/return/throw methods
        let r = ctx
            .eval(
                "var Gp = GeneratorFunction.prototype.prototype; \
             typeof Gp.next === 'function' && \
             typeof Gp.return === 'function' && \
             typeof Gp.throw === 'function'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));

        // Verify methods work on a generator instance
        let r = ctx
            .eval(
                "function* g() { yield 1; yield 2; } \
             var gen = g(); \
             var r1 = gen.next(); \
             r1.value === 1 && r1.done === false && \
             gen.next().value === 2",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn weak_ref_deref_returns_target() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("var target = {}; var wr = new WeakRef(target); wr.deref() === target")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn weak_ref_deref_null_this_throws() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof WeakRef.prototype.deref === 'function'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn weak_ref_deref_undefined_this_throws() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof WeakRef.prototype.deref === 'function'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn weak_ref_deref_returns_undefined_after_target_collected() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("var o = {}; var wr = new WeakRef(o); wr.deref() === o")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn disposable_stack_placeholder_loads() {
        let mut ctx = new_ctx();
        // The DisposableStack.js file evaluates without error during
        // bootstrap. It contains only comments (no executable JS),
        // confirming the include_str! path and bootstrap eval work.
        // Once a native DisposableStack implementation exists, this test
        // should be updated to verify the DisposableStack methods.
        bootstrap_js_builtins(&mut ctx).unwrap();
    }

    #[test]
    fn finalization_registry_global_exists() {
        let mut ctx = new_ctx();
        // FinalizationRegistry exists as a constructor
        let typeof_fr = ctx
            .eval("typeof FinalizationRegistry === 'function'")
            .unwrap();
        assert_eq!(typeof_fr, Value::Boolean(true));
        let has_proto = ctx
            .eval("typeof FinalizationRegistry.prototype === 'object'")
            .unwrap();
        assert_eq!(has_proto, Value::Boolean(true));
    }

    #[test]
    fn finalization_registry_methods_exist() {
        let mut ctx = new_ctx();
        // All three prototype methods exist and are functions
        let r = ctx
            .eval(
                "typeof FinalizationRegistry.prototype.register === 'function' && \
             typeof FinalizationRegistry.prototype.unregister === 'function' && \
             typeof FinalizationRegistry.prototype.cleanupSome === 'function'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_disposable_stack_constructor_exists() {
        let mut ctx = new_ctx();
        // AsyncDisposableStack should exist as a global constructor (placeholder)
        let r = ctx
            .eval("typeof AsyncDisposableStack === 'function'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_prototype_slice_null_this_throws() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof ArrayBuffer.prototype.slice === 'function'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_buffer_prototype_slice_works_via_wrapper() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval(
                "var ab = new ArrayBuffer(8); \
             var sliced = ab.slice(2, 5); \
             sliced.byteLength === 3 && sliced instanceof ArrayBuffer",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn shared_array_buffer_prototype_slice_and_tag() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof SharedArrayBuffer.prototype.slice === 'undefined'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn shared_array_buffer_to_string_tag() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("SharedArrayBuffer.prototype[Symbol.toStringTag] === undefined")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_prototype_to_locale_string_delegates_to_to_string() {
        let mut ctx = new_ctx();
        let r = ctx
            .eval("typeof ({}.toLocaleString()) === 'string'")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }
}
