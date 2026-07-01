# Task 64: Implement NaN-boxed Value type

## Status: Pending

## Goal

Implement NaN-boxed Value type for quench-runtime as the foundation for performance optimizations.

## Current State

- Value type is defined in `value.rs`
- Uses `Box`/`Rc` for heap allocation
- Value is not `Copy` (expensive to pass around)

## Target

Implement NaN-boxed Value following the ECMAScript standard approach:

```rust
// NaN-boxed Value representation using u64
// Layout: top 16 bits are tag, bottom 48 bits are payload
//
// Tag 0xFFF0: undefined
// Tag 0xFFF1: null  
// Tag 0xFFF2: true
// Tag 0xFFF3: false
// Tag 0xFFF4: integer (31-bit signed)
// Tag 0xFFFF: pointer (object/string/function)
// Any other NaN: f64
//
// Special values:
// - Undefined: all bits 0 (matches JS semantics)
// - Null: tag 0xFFF1
// - Booleans: tags 0xFFF2 and 0xFFF3
// - Integers: tag 0xFFF4 with 32-bit signed value
// - Objects/strings/functions: tag 0xFFFF with pointer
// - Floats: any other encoding where top bits indicate NaN
```

## Requirements

1. Value must be `Copy`
2. All existing functionality must be preserved
3. Tests must pass after the change
4. Add unit tests for NaN-boxing edge cases

## Approach

1. Create `value/nanbox.rs` module with the Value type
2. Implement `From`/`Into` for f64, i32 conversions
3. Implement display/debug representations
4. Ensure object pointers are 8-byte aligned (required for NaN-boxing)
5. Update existing code to use the new Value type
6. Run tests after each change

## TDD Steps

1. Write failing tests for `Value::is_undefined`, `Value::is_null`
2. Write tests for f64 <-> Value round-tripping
3. Write tests for integer <-> Value round-tripping
4. Write tests for pointer encoding/decoding
5. Implement the NaN-boxing logic
6. Run full test suite

## Performance Target

Value passing should be as cheap as passing a `u64`.

## Rationale

The goal is to lay the foundation for future optimizations:
- String interning will work naturally with NaN-boxed pointers
- Object shapes (hidden classes) can be implemented on top
- Slot-indexed environments become more efficient

## Deferral Note

This task may be deferred to a future phase if the architecture split (Task 63) takes priority. The current Value implementation works correctly; this is purely a performance optimization.

## Verification

```bash
cargo test -p quench-runtime
cargo bench
```

## Dependencies

- Task 63 (architecture split) should be completed first for cleaner code organization
