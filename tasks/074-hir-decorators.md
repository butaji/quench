# Task 074: Add Decorators to HIR + Parser (Type Erasure)

**Priority:** P3-Low
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 073

## Problem

Decorators (`@decorator`) are parsed by oxc but not represented in HIR.

## Work

### 1. Verify HIR support ✅

Check if `Decorator` struct in HIR is populated.

### 2. Update parser ✅

Handle `Decorator` nodes in class/method declarations.

Added `decorators: Vec<Decorator>` field to:
- `ClassDecl` struct
- `ClassMethod` struct
- `ClassMember` struct

Updated `stmt_class.rs` parser to:
- Extract decorators from `Class`, `MethodDefinition`, and `PropertyDefinition`
- Convert oxc `Decorator` expressions to HIR using `convert_expr`

### 3. Update codegen ✅

For v1, decorators are erased (no runtime code).
The codegen simply ignores the `decorators` field since it only generates Rust structs/impls.

## Acceptance Criteria

- [x] Decorators parse into HIR without producing `Invalid`
- [x] Codegen erases decorators (no code generated)
- [x] Parser tests added: `class_with_decorator`, `class_with_method_decorator`, `class_with_call_decorator`

## Changes Made

- `crates/runts-hir/src/base.rs`: Added `decorators` field to ClassDecl, ClassMethod, ClassMember
- `src/transpile/parser/stmt_class.rs`: Added `convert_decorators()` and `convert_decorator_expr()` functions
- `src/transpile/tests/spec_classes.rs`: Added 3 new tests for decorator parsing
