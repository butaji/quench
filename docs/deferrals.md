# Deferred Items

This document tracks postponed features and design decisions for the Quench runtime.

## High Priority (Should Address Soon)

### 1. JSX/TSX Support
- **Status**: Not implemented
- **Blocking**: Examples like `use-bridge.tsx`, `animations.tsx`
- **Effort**: Medium-High
- **Notes**: Need to add JSX parsing and transformation to the swc pipeline

### 2. ES Module Import/Export
- **Status**: Partial (ExportDefaultExpr handled, others return None)
- **Blocking**: Many TypeScript examples use import/export
- **Effort**: Medium
- **Notes**: Need to implement full ES module support with import statements and dynamic imports

### 3. Async/Await & Promises
- **Status**: Not implemented
- **Blocking**: Timer effects, async operations in examples
- **Effort**: High
- **Notes**: Need event loop integration for microtasks and async function support

### 4. Iterator/Generator Support
- **Status**: Partial (for-of loops work for arrays)
- **Blocking**: Spread operators on iterables, Set/Map iteration
- **Effort**: Medium
- **Notes**: Need proper Symbol.iterator support and generator functions

### 5. Symbol Support
- **Status**: Missing Symbol primitive
- **Blocking**: Well-known symbols, Symbol.iterator
- **Effort**: Medium
- **Notes**: Need to implement Symbol as a primitive type

## Medium Priority

### 6. Getters/Setters on Objects
- **Status**: Not implemented
- **Blocking**: Some JavaScript patterns
- **Effort**: Medium
- **Notes**: Need to implement accessor property evaluation

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
- **Status**: Currently recursive
- **Issue**: Stack overflow in complex programs
- **Effort**: High
- **Notes**: An agent was started on this but timed out

### B. File Size Limits Refactoring
- **Status**: Several files exceed 500 lines
- **Files**: builtins/array.rs (547), interpreter/eval_expr.rs (585), lower/expressions.rs (467), lower/statements.rs (464), builtins/string.rs (454)
- **Effort**: Medium
- **Notes**: Need to split these into smaller modules
