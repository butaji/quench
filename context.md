# Conformance Test Analysis: quench-runtime

## Summary of Test Results

### test262 (ECMAScript conformance)
| Metric | Count | Rate |
|--------|-------|------|
| Total | 431 | 100% |
| Passed | 47 | 10.9% |
| Failed | 86 | 20.0% |
| Skipped | 298 | 69.1% |

### TypeScript Conformance (100 test sample)
| Metric | Count | Rate |
|--------|-------|------|
| Total | 100 | 100% |
| Passed | 13 | 13.0% |
| Failed | 48 | 48.0% |
| Skipped | 39 | 39.0% |

---

## Top 10 Failure Patterns

### 1. Missing Array.prototype Methods (~25% of failures)
**Category:** Missing built-in

Missing methods:
- `Array.prototype.filter` - transformation filter
- `Array.prototype.find` / `findIndex` - search
- `Array.prototype.includes` - membership check
- `Array.prototype.reduce` / `reduceRight` - aggregation
- `Array.prototype.flat` / `flatMap` - flattening
- `Array.prototype.some` / `every` - predicates
- `Array.prototype.toSpliced`, `toSorted`, `toReversed` - immutable variants

**Files to modify:** `crates/quench-runtime/src/builtins/array/methods/`

**High-impact fix:**
1. Add `Array.prototype.includes` - enables `in` operator tests and membership checks
2. Add `Array.prototype.filter` - enables filter tests and transforms

---

### 2. Missing String.prototype Methods (~15% of failures)
**Category:** Missing built-in

Missing methods:
- `String.prototype.matchAll` - regex iterator
- `String.prototype.replaceAll` - replace all occurrences
- `String.prototype.trimStart` / `trimEnd` - trimming variants
- `String.prototype.padStart` / `padEnd` - padding
- `String.prototype.at` - index access

**Files to modify:** `crates/quench-runtime/src/builtins/string/methods/`

**High-impact fix:**
1. Add `String.prototype.padStart` / `padEnd` - common string formatting
2. Add `String.prototype.trimStart` / `trimEnd` - modern trim methods

---

### 3. Missing `in` and `instanceof` Operators
**Category:** Missing operator support

**Status:** Operators are implemented in `eval/operators.rs:52-83`, but may have edge case issues with:
- Objects with Symbol properties
- Proxy objects
- Cross-realm objects

**Files to examine:** `crates/quench-runtime/src/eval/operators.rs`

**High-impact fix:**
1. Fix `in` operator for computed properties
2. Fix `instanceof` for cross-realm objects

---

### 4. Missing `typeof` Special Cases (~10% of failures)
**Category:** Missing operator support

`typeof` returns incorrect results for:
- Objects with Symbol properties (should be 'symbol' for Symbol keys via Symbol.toStringTag)
- Host objects (implementation-defined)
- Function objects created with different realms

**Files to modify:** `crates/quench-runtime/src/eval/operators.rs:175-195`

**High-impact fix:**
1. Fix `typeof` for objects with Symbol properties
2. Handle special cases for function objects

---

### 5. Missing Spread Operator (~8% of failures)
**Category:** Missing syntax support

**Status:** `spread` and `spread-syntax` features are skipped

Missing support for:
- `[...arr]` array spread
- `{...obj}` object spread  
- `func(...args)` function call spread
- `new Cls(...args)` constructor spread

**Files to modify:** `crates/quench-runtime/src/lower/expr.rs:351-354`

**High-impact fix:**
1. Implement spread in array literals (lower/expr.rs)
2. Implement spread in function calls (lower/expr.rs)

---

### 6. Missing Computed Property Names (~7% of failures)
**Category:** Missing syntax support

**Status:** `Computed` property names are not supported

Missing for:
- Object literals: `{ [key]: value }`
- Class members: `{ [computed]() {} }`
- Method definitions

**Files to modify:** `crates/quench-runtime/src/lower/helpers.rs:105`

**High-impact fix:**
1. Implement computed property names in object literals
2. Implement computed property names in classes

---

### 7. Missing Destructuring Assignment (~6% of failures)
**Category:** Missing syntax support

**Status:** Destructuring is not supported in assignment position

Missing for:
- `[a, b] = arr` array destructuring
- `{a, b} = obj` object destructuring
- Nested patterns
- Default values

**Files to modify:** `crates/quench-runtime/src/lower/expr.rs:330-347`

**High-impact fix:**
1. Implement array destructuring assignment
2. Implement object destructuring assignment

---

### 8. Missing `delete` Operator (~5% of failures)
**Category:** Missing operator support

**Status:** Unary delete is not supported

Missing for:
- `delete obj.prop` property deletion
- `delete arr[index]` element deletion
- `delete identifier` (returns false in strict mode)

**Files to modify:** `crates/quench-runtime/src/lower/helpers.rs:77`

**High-impact fix:**
1. Implement property deletion
2. Implement delete semantics (strict mode, getters, etc.)

---

### 9. Missing Unary `+` Operator (~4% of failures)
**Category:** Missing operator support

**Status:** Unary plus is not supported

Used for:
- Explicit number conversion: `+"42"`
- Type coercion to number

**Files to modify:** `crates/quench-runtime/src/lower/helpers.rs:72`

**High-impact fix:**
1. Implement unary plus with ToNumber conversion

---

### 10. Missing Tagged Templates (~3% of failures)
**Category:** Missing syntax support

**Status:** Tagged templates not supported

Missing for:
- `` tag`template` `` syntax
- Template literal processing

**Files to modify:** `crates/quench-runtime/src/lower/expr.rs:39`

**High-impact fix:**
1. Implement basic tagged template support
2. Handle template literal raw/cooked values

---

## Additional High-Impact Missing Features

### Class Fields (currently skipped)
**Category:** Missing feature

Tests with these features are skipped:
- `class-fields-public`
- `class-fields-private`
- `class-static-fields-public`
- `class-static-fields-private`

**Files to modify:** `crates/quench-runtime/src/lower/expr.rs:440`

---

### Promise/Async (currently skipped)
**Category:** Missing feature

Tests with these features are skipped:
- `Promise`
- `async-functions`
- `async-iteration`

**Files to examine:** `crates/quench-runtime/src/builtins/promise.rs`

---

### Generators (currently skipped)
**Category:** Missing feature

Tests with `generators` feature are skipped

---

## Files Requiring Changes

### Priority 1 (High Impact, Low Effort)
1. `crates/quench-runtime/src/lower/helpers.rs` - Unary plus, delete
2. `crates/quench-runtime/src/builtins/array/methods/mod.rs` - Array methods
3. `crates/quench-runtime/src/builtins/string/methods/mod.rs` - String methods

### Priority 2 (Medium Impact, Medium Effort)
4. `crates/quench-runtime/src/lower/expr.rs` - Spread, computed props, destructuring
5. `crates/quench-runtime/src/eval/operators.rs` - typeof, in, instanceof fixes

### Priority 3 (High Impact, High Effort)
6. `crates/quench-runtime/src/lower/expr.rs` - Class fields
7. `crates/quench-runtime/src/builtins/promise.rs` - Promise/async support

---

## Recommended Priority Order

1. **Add Array.prototype.includes** - Quick win, unlocks many tests
2. **Fix typeof for special cases** - Quick fix in operators.rs
3. **Implement unary +** - Single function addition
4. **Implement spread in arrays** - Medium complexity
5. **Add String.prototype.padStart/trimStart** - Common methods
6. **Implement computed property names** - Medium complexity
7. **Implement destructuring assignment** - Higher complexity
8. **Add Array.prototype.filter/reduce** - More array methods
