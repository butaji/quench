# Task 078: Add Generator Functions `function*` to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 077

## Problem

Generator functions (`function*`) are not represented in HIR. Parser handles `generator` flag but only sets it on `FunctionDecl`; the body is not processed.

## Work

### 1. Verify HIR support

HIR already has `FunctionDecl.is_generator` and `Expr::Yield`. The gap is in parser body processing.

### 2. Update parser

Process generator function bodies in `conv_func_expr` and `stmt_decl.rs`.

### 3. Update codegen

Map generators to Rust iterators:
```rust
// function* range(start, end) { for (let i = start; i <= end; i++) yield i; }
// →
fn range(start: i32, end: i32) -> impl Iterator<Item = i32> {
    (start..=end).into_iter()
}
```

## Acceptance Criteria

- [ ] `function*` declarations parse into HIR with complete bodies
- [ ] `yield` and `yield*` produce compilable Rust
- [ ] Generators map to Rust iterators
- [ ] `ink-generator` example (Task 057) passes parity harness