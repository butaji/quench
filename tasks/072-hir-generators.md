# Task 072: Add Generator Functions `function*` to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 071

## Problem

Generator functions (`function*`) are not fully represented in HIR. Parser sets `is_generator` but body is not processed.

## Work

### 1. Verify HIR support

HIR already has `FunctionDecl.is_generator` and `Expr::Yield`. The gap is in parser body processing.

### 2. Update parser

Process generator function bodies in `conv_func_expr` and `stmt_decl.rs`.

### 3. Update codegen

Map generators to Rust iterators.

## Acceptance Criteria

- [ ] `function*` with `yield`/`yield*` parses into HIR with complete bodies
- [ ] Codegen maps generators to Rust iterators
- [ ] `ink-generator` example (Task 053) passes with 100% parity
