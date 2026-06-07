# Task 038: Fix Compile-Path Codegen for Expressions

**Priority:** P1-High
**Phase:** 5 — Compile Path Hardening
**Depends on:** 037

## Problem

Compile-path codegen was missing or broken for expressions: template literals, logical operators (`&&`, `||`, `??`), compound assignment (`+=`, `-=`, etc.), spread, `typeof`.

## Acceptance Criteria

- [x] Template literals produce compilable Rust
- [x] Logical operators produce compilable Rust
- [x] All 12 compound assignment operators produce compilable Rust
- [x] Array/object spread produces compilable Rust
- [x] `typeof` constant-folding works
- [x] At least 5 compile-path integration tests added
- [x] `cargo build` passes with 0 errors
