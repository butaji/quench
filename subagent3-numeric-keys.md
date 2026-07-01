# Subagent 3: Numeric Keys and for...in Fixes

## Task Summary

Fixed Issue 1: Numeric-string keys on ordinary objects in quench-runtime.

Issue 2 (for...in): **Not applicable** - The codebase does not have for...in implemented. The lowerer (`lower.rs`) returns `None` for `ForIn` statements, meaning the feature is not supported. The task description referenced `loops.rs` but the codebase has `interpreter.rs` - this indicates the task was written for a different version of the codebase.

## Issue 1: Numeric-string Keys on Ordinary Objects

### Problem
`Object::set()` was storing numeric string keys (e.g., `obj['123']`) in the `elements` vector for ALL objects, not just Array objects. This caused ordinary objects to incorrectly gain array-like behavior (expanding `elements` vector and setting `length` property).

### Root Cause
In `crates/quench-runtime/src/value.rs`, the `set()` method unconditionally checked if a key was a numeric string and stored it in `elements`:
```rust
pub fn set(&mut self, key: &str, value: Value) {
    if let Ok(idx) = key.parse::<usize>() {
        while self.elements.len() <= idx {
            self.elements.push(Value::Undefined);
        }
        self.elements[idx] = value.clone();
        self.properties.insert("length".to_string(), Value::Number(self.elements.len() as f64));
    }
    self.properties.insert(key.to_string(), value);
}
```

### Fix
Modified both `get()` and `set()` to only use array storage for `ObjectKind::Array` objects:

```rust
// In get():
if self.kind == ObjectKind::Array {
    if let Ok(idx) = key.parse::<usize>() {
        if idx < self.elements.len() {
            return Some(self.elements[idx].clone());
        }
    }
}

// In set():
if self.kind == ObjectKind::Array {
    if let Ok(idx) = key.parse::<usize>() {
        while self.elements.len() <= idx {
            self.elements.push(Value::Undefined);
        }
        self.elements[idx] = value.clone();
        self.properties.insert("length".to_string(), Value::Number(self.elements.len() as f64));
        return;
    }
}
```

## Validation

### Before Fix
- `obj['123'] = x` on an ordinary object would expand `elements` and set `length`
- This is incorrect per ECMAScript semantics

### After Fix
- Ordinary objects store numeric keys in `properties` HashMap only
- Array objects continue to use `elements` vector for numeric keys
- `length` property is only set on Array objects

### Test Results
```
cargo test -p quench-runtime ... ok. 20 passed; 0 failed
cargo test -p quench-runtime --test runtime ... ok. 1 passed
```

## Changed Files

1. `crates/quench-runtime/src/value.rs` - Fixed `get()` and `set()` methods

## Notes

- The codebase uses monolithic files (`value.rs`, `interpreter.rs`) rather than split modules (`value/mod.rs`, etc.) as referenced in the task description
- for...in loop support is not implemented in this codebase (feature returns None in lowerer)
- The codebase appears to be in an inconsistent state with some pre-existing compilation errors when certain code paths are exercised
