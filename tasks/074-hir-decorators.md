# Task 074: Add Decorators to HIR + Parser (Type Erasure)

**Priority:** P3-Low
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 073

## Problem

Decorators (`@decorator`) are parsed by oxc but not represented in HIR.

## Work

### 1. Verify HIR support

Check if `Decorator` struct in HIR is populated.

### 2. Update parser

Handle `Decorator` nodes in class/method declarations.

### 3. Update codegen

For v1, decorators are erased (no runtime code).

## Acceptance Criteria

- [ ] Decorators parse into HIR without producing `Invalid`
- [ ] Codegen erases decorators
- [ ] Parser tests added
