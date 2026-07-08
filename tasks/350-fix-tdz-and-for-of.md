# Task 350: Fix TDZ handling and for-of loop

## Status: COMPLETED

## Priority: P0 correctness

## Problem

Two related issues were causing TDZ (temporal dead zone) errors:

### Issue 1: TDZ declarations collected from nested blocks

The `predeclare_let_const` function was collecting let/const declarations from ALL nested blocks and marking them as TDZ in the current scope. This caused incorrect TDZ errors when accessing variables in nested blocks.

**Example that was broken:**
```javascript
{
  const outer = 1;
  {
    const inner = 2;
    console.log(outer);  // TDZ error!
    console.log(inner);
  }
}
```

### Issue 2: For-of loop variables not re-initialized

The for-of loop implementation was not properly re-initializing loop variables on each iteration. After the first iteration, subsequent iterations would see the first value instead of the current iteration's value.

**Example that was broken:**
```javascript
for (const x of [10, 20, 30]) {
  console.log(x);  // Output: 10, 10, 10 (should be 10, 20, 30)
}
```

## Root Cause

### Issue 1: Incorrect recursion in predeclare_let_const

The `collect_let_const_recursive` function was recursing into nested blocks and collecting all declarations into a single list. When deduplicated, this incorrectly shadowed outer declarations.

### Issue 2: initialize_declared only checked declarations, not bindings

The `Environment::initialize_declared` function only looked for variables in `declarations`, but after the first iteration, the variable was moved from `declarations` to `bindings`. Subsequent iterations would not find the variable and would fall through to the parent environment.

## Fix

### Fix 1: Only collect top-level declarations

Replaced `collect_let_const_declarations` and `collect_let_const_recursive` with `collect_toplevel_let_const` that only collects declarations at the current scope level, not from nested blocks.

### Fix 2: Check both declarations and bindings

Modified `Environment::initialize_declared` to check both `declarations` and `bindings` when finding the scope for a variable.

### Fix 3: For-of loop variable declaration

Modified the for-of and for-in implementations to properly declare loop variables with `VarKind::Let` before initializing them on each iteration.

## Files Changed

- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/env.rs`

## Verification

```bash
cargo test -p quench-runtime  # All tests pass
```

### Test cases now working:

```javascript
// Nested block TDZ
{
  const outer = 1;
  {
    const inner = 2;
    console.log(outer);  // Works
    console.log(inner);   // Works
  }
}

// For-of loop
for (const x of [10, 20, 30]) {
  console.log(x);  // Outputs: 10, 20, 30
}

// For-of in function
function test(items) {
  for (const x of items) {
    console.log(x);
  }
}
test([1, 2, 3]);  // Outputs: 1, 2, 3
```

## Test Files

- `crates/quench-runtime/tests/var_hoisting_tdz.rs` - TDZ tests
- `crates/quench-runtime/tests/scenarios.rs` - JS scenario tests

## Known Limitations

The stack overflow issue in examples (e.g., counter.js) is a separate architectural problem. The interpreter is recursive and uses the native Rust call stack, which has limited size. This is tracked in Task 85 (trampoline interpreter) and Task 343 (fix stack overflow).
