# Task 040: Fix Compile-Path Codegen for Data Structures

**Priority:** P1-High
**Phase:** 5 — Compile Path Hardening
**Depends on:** 039

## Problem

Compile-path codegen was missing or broken for data structures: destructuring, object spread, rest patterns, multi-declarators.

## Acceptance Criteria

- [x] Object/array destructuring with defaults produces compilable Rust
- [x] Rest patterns produce compilable Rust
- [x] Object spread produces compilable Rust
- [x] Multiple variable declarators produce compilable Rust
- [x] `for-of` with destructuring LHS produces compilable Rust
- [x] At least 5 compile-path integration tests added
- [x] `cargo build` passes with 0 errors
