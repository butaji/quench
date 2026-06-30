# Task 13: Fix Array.prototype.flat bug and missing globals

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Status: COMPLETED

## Issues Fixed

### 1. Array.prototype.flat - Non-array objects dropped

**Problem**: When `flat()` encountered an array containing non-array objects (like `[{type: 'Box'}]`), the objects were being dropped because the implementation only iterated over `obj.elements` for non-array objects.

**Fix**: Modified `flatten_depth()` in `crates/quench-runtime/src/builtins/array_methods.rs` to check if the object is an array before iterating elements, and push the object itself if it's not an array:

```rust
} else if obj.kind == ObjectKind::Array {
    // At max depth for an array, just copy elements
    for elem in &obj.elements {
        result.push(elem.clone());
    }
} else {
    // Not an array, push the object itself
    result.push(val.clone());
}
```

### 2. Array.prototype.flat - Missing prototype chain

**Problem**: `flat()` returned a new array without setting up the prototype chain. This caused method chaining like `flat().filter()` to fail because `filter` wasn't found on the prototype.

**Fix**: 
1. Added thread-local storage in `crates/quench-runtime/src/builtins/array.rs` to store the `Array.prototype` reference:
```rust
thread_local! {
    static ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = RefCell::new(None);
}
```

2. Updated `make_flat()` in `crates/quench-runtime/src/builtins/array_methods.rs` to set the prototype on the returned array:
```rust
let mut arr = Object::new_array_from(items);
if let Some(proto) = get_array_prototype() {
    arr.prototype = Some(proto);
}
```

### 3. Missing global constants

**Problem**: `Infinity`, `NaN`, and `undefined` were used in `runtime.js` but not defined.

**Fix**: Added to `crates/quench-runtime/src/builtins/global.rs`:
```rust
ctx.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
ctx.set_global("NaN".to_string(), Value::Number(f64::NAN));
ctx.set_global("undefined".to_string(), Value::Undefined);
```

## Verification

- All 46 unit tests pass
- All 3 parity tests pass
- All 60+ example apps run without errors
- No lint warnings in quench-runtime crate
