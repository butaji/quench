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

/// Embedded JS builtin source files.
/// Each is loaded via `include_str!` at compile time and evaluated in order.
const BUILTIN_FILES: &[(&str, &str)] = &[
    // Phase 1: core intrinsics (once _intrinsics.js exists)
    // ("_intrinsics", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builtins/_intrinsics.js"))),
    // Phase 2: Object
    ("Object", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builtins/Object.js"))),
    // Phase 3: Array
    ("Array", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builtins/Array.js"))),
    // Phase 3+: add in dependency order
];

/// Evaluate all self-hosted JS builtin files in dependency order.
/// Called once per `Context::new()` after `init_builtins` registers `__ops__`.
pub fn bootstrap_js_builtins(ctx: &mut Context) -> Result<(), JsError> {
    for (name, source) in BUILTIN_FILES {
        ctx.eval(source).map_err(|e| {
            JsError(format!("bootstrap {}: {}", name, e))
        })?;
    }
    Ok(())
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
        let r = ctx.eval(
            "var a = {}; var b = {}; \
             Object.is(a, a) && !Object.is(a, b)",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn bootstrap_runs_without_error() {
        let mut ctx = Context::new().unwrap();
        bootstrap_js_builtins(&mut ctx).unwrap();
    }

    #[test]
    fn object_keys_returns_own_enumerable_keys() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var o = { a: 1, b: 2 }; \
             var keys = Object.keys(o); \
             keys.length === 2 && keys[0] === 'a' && keys[1] === 'b'",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_does_not_include_inherited() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var proto = { p: 1 }; \
             var o = Object.create(proto); \
             o.own = 2; \
             Object.keys(o).length === 1 && Object.keys(o)[0] === 'own'",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_non_enumerable_not_included() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var o = {}; \
             Object.defineProperty(o, 'hidden', { value: 1, enumerable: false }); \
             o.visible = 2; \
             Object.keys(o).length === 1 && Object.keys(o)[0] === 'visible'",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_keys_primitives_return_empty() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "Object.keys(42).length === 0",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_values_returns_values() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var o = { a: 1, b: 2 }; \
             var vals = Object.values(o); \
             vals.length === 2 && vals[0] === 1 && vals[1] === 2",
        ).unwrap();
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
        let r = ctx.eval(
            "var o = { a: 1, b: 2 }; \
             var ents = Object.entries(o); \
             ents.length === 2 && ents[0][0] === 'a' && ents[0][1] === 1 && \
             ents[1][0] === 'b' && ents[1][1] === 2",
        ).unwrap();
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
        let r = ctx.eval(
            "var target = { a: 1 }; \
             var source = { b: 2, c: 3 }; \
             var result = Object.assign(target, source); \
             result === target && target.a === 1 && target.b === 2 && target.c === 3",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_overwrites_properties() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var target = { a: 1, b: 2 }; \
             var source = { b: 3, c: 4 }; \
             Object.assign(target, source); \
             target.a === 1 && target.b === 3 && target.c === 4",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_skips_null_source() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var target = { a: 1 }; \
             Object.assign(target, null, { b: 2 }); \
             target.a === 1 && target.b === 2",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_multiple_sources() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var target = {}; \
             Object.assign(target, { a: 1 }, { b: 2 }, { c: 3 }); \
             target.a === 1 && target.b === 2 && target.c === 3",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_assign_returns_target() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var target = {}; \
             Object.assign(target, { a: 1 }) === target",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_has_own_own_property() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var o = { a: 1 }; \
             Object.hasOwn(o, 'a') && !Object.hasOwn(o, 'b')",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_has_own_inherited_not_found() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var proto = { p: 1 }; \
             var o = Object.create(proto); \
             Object.hasOwn(o, 'p')",
        ).unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn object_has_own_primitive() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "Object.hasOwn(42, 'toString')",
        ).unwrap();
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
        let r = ctx.eval(
            "var o = {}; \
             Object.preventExtensions(o); \
             !Object.isExtensible(o)",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_from_entries_creates_object() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var entries = [['a', 1], ['b', 2]]; \
             var obj = Object.fromEntries(entries); \
             obj.a === 1 && obj.b === 2",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_from_entries_empty() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var obj = Object.fromEntries([]); \
             Object.keys(obj).length === 0",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_is_array_true() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Array.isArray([]) && Array.isArray(new Array(5))").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_is_array_false() {
        let mut ctx = new_ctx();
        let r = ctx.eval("!Array.isArray({}) && !Array.isArray(null) && !Array.isArray(42)").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_forEach_iterates() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = []; \
             [1, 2, 3].forEach(function(v) { result.push(v); }); \
             result.length === 3 && result[0] === 1 && result[1] === 2 && result[2] === 3",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_map_transforms() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = [1, 2, 3].map(function(v) { return v * 2; }); \
             result.length === 3 && result[0] === 2 && result[1] === 4 && result[2] === 6",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_filter_filters() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = [1, 2, 3, 4, 5].filter(function(v) { return v % 2 === 0; }); \
             result.length === 2 && result[0] === 2 && result[1] === 4",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reduce_sums() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3, 4, 5].reduce(function(acc, v) { return acc + v; }, 0) === 15",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reduce_without_initial() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].reduce(function(acc, v) { return acc + v; }) === 6",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_finds_element() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var result = [1, 2, 3, 4, 5].find(function(v) { return v > 3; }); \
             result === 4",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_find_returns_undefined() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].find(function(v) { return v > 10; }) === undefined",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_some_returns_true() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].some(function(v) { return v === 2; })",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_some_returns_false() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "![1, 2, 3].some(function(v) { return v === 5; })",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_every_returns_true() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].every(function(v) { return v > 0; })",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_every_returns_false() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "![1, 2, 3].every(function(v) { return v > 1; })",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_includes_finds_element() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].includes(2)",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_includes_not_found() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "![1, 2, 3].includes(5)",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_indexOf_finds_index() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].indexOf(2) === 1",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_indexOf_not_found() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].indexOf(5) === -1",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_join_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].join('-') === '1-2-3'",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_join_default_separator() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].join() === '1,2,3'",
        ).unwrap();
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
        let r = ctx.eval(
            "var arr = [1, 2, 3]; \
             var popped = arr.pop(); \
             popped === 3 && arr.length === 2 && arr[0] === 1 && arr[1] === 2",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_slice_works() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [1, 2, 3, 4, 5]; \
             var sliced = arr.slice(1, 3); \
             sliced.length === 2 && sliced[0] === 2 && sliced[1] === 3 && arr.length === 5",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_concat_combines() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var a = [1, 2]; \
             var b = [3, 4]; \
             var c = a.concat(b); \
             c.length === 4 && c[0] === 1 && c[1] === 2 && c[2] === 3 && c[3] === 4",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reverse_reverses() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [1, 2, 3]; \
             arr.reverse(); \
             arr.length === 3 && arr[0] === 3 && arr[1] === 2 && arr[2] === 1",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_fill_fills_range() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var arr = [1, 2, 3, 4]; \
             arr.fill(0, 1, 3); \
             arr[0] === 1 && arr[1] === 0 && arr[2] === 0 && arr[3] === 4",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_reduceRight_reduces_reverse() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "[1, 2, 3].reduceRight(function(acc, v) { return acc + String(v); }, '') === '321'",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_keys_returns_indices() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var k = ['a', 'b', 'c'].keys(); \
             k.length === 3 && k[0] === 0 && k[1] === 1 && k[2] === 2",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn array_values_returns_values() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var v = ['a', 'b'].values(); \
             v.length === 2 && v[0] === 'a' && v[1] === 'b'",
        ).unwrap();
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
}
