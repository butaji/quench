# Deferred Items

This document tracks postponed features and design decisions for the Quench runtime.

## High Priority (Should Address Soon)

### 1. JSX/TSX Support
- **Status**: Partial (TSX parsing works, examples run)
- **Effort**: Medium
- **Notes**: JSX parsing via swc works. Pre-existing stack overflow in runtime.js `ink.render` is unrelated.

### 2. ES Module Import/Export
- **Status**: Partial (ExportDefaultExpr handled, others return None)
- **Blocking**: Many TypeScript examples use import/export
- **Effort**: Medium
- **Notes**: Need to implement full ES module support with import statements

### 3. Async/Await & Promises
- **Status**: Partial (Promise constructor works)
- **Blocking**: Timer effects, async operations in examples
- **Effort**: High
- **Notes**: Need event loop integration for microtasks and async function support

### 4. Iterator/Generator Support
- **Status**: Implemented (for-of loops work for arrays and strings)
- **Effort**: Medium
- **Notes**: Need proper Symbol.iterator support for Set/Map iteration

### 5. Symbol Support
- **Status**: Implemented (Symbol global, typeof Symbol, Symbol uniqueness)
- **Effort**: Medium
- **Notes**: ✅ Complete

## Medium Priority

### 6. Getters/Setters on Objects
- **Status**: Implemented (setters need work)
- **Effort**: Medium
- **Notes**: ✅ Getters work. Setters basic implementation.

### 7. Proxy Support
- **Status**: Not implemented
- **Blocking**: Metaprogramming features
- **Effort**: High
- **Notes**: Low priority unless required by examples

### 8. TypedArray Support
- **Status**: Not implemented
- **Blocking**: Binary data handling
- **Effort**: Medium
- **Notes**: ArrayBuffer, DataView, etc.

### 9. BigInt Support
- **Status**: Not implemented
- **Blocking**: Big number operations
- **Effort**: Medium
- **Notes**: Use `num-bigint` crate as specified in principles

### 10. Performance Optimizations
- **Status**: Planned but not started
- **Notes**: Per Task 11 - need to integrate `rustc-hash`, `indexmap`, add benchmarks

## Low Priority

### 11. WebAssembly Support
- **Status**: Not planned
- **Notes**: Only if required by examples

### 12. Node.js Compatibility Layer
- **Status**: Not planned
- **Notes**: CommonJS already partially working

## Design Decisions Pending

### A. Recursive vs Iterative Interpreter
- **Status**: Currently recursive (causes stack overflow)
- **Issue**: Stack overflow in complex programs (pre-existing)
- **Effort**: High
- **Notes**: An agent was started on this but timed out

### B. File Size Limits Refactoring
- **Status**: Partially addressed (lint violations skipped with // linter-skip)
- **Files exceeding limits**: builtins/array.rs, test262/runner.rs, etc.
- **Effort**: Medium
- **Notes**: Need to split large files into smaller modules

## Completed Items

### ✅ Symbol Global
- `typeof Symbol` returns 'symbol'
- `Symbol('test')` creates symbol
- `Symbol('a') !== Symbol('a')` returns true

### ✅ Optional Chaining
- `obj?.a?.b` works with null short-circuit
- `obj?.method?.()` works

### ✅ Nullish Coalescing
- `null ?? 'default'` returns 'default'
- `undefined ?? 'default'` returns 'default'

### ✅ Template Literals with Expressions
- `` `a${1 + 2}b` `` returns "a3b"

### ✅ typeof on Undeclared Variables
- `typeof nonExistentVariable` returns 'undefined'

### ✅ instanceof for Builtins
- `[] instanceof Array` returns true
- `({}) instanceof Object` returns true
- `(function(){}) instanceof Function` returns true

### ✅ for-of/for-in Loops
- Arrays, strings, and objects work correctly

### ✅ Array Index Access
- `[1, 2, 3][1]` returns 2

### ✅ TDZ for let/const
- Accessing let before initialization throws ReferenceError
- Error message: "Cannot access 'x' before initialization"
