# Task 037: Fix Compile-Path Codegen for Control Flow

**Priority:** P1-High
**Phase:** 5 — Compile Path Hardening
**Depends on:** 036

## Problem

Compile-path codegen was missing or broken for control flow constructs: `for`, `switch`, `try/catch`, `throw`, `break`, `continue`.

## Acceptance Criteria

- [x] `for` loops produce compilable Rust
- [x] `while`/`do-while` produce compilable Rust
- [x] `switch` statements produce compilable Rust
- [x] `try`/`catch`/`finally` produce compilable Rust
- [x] `throw` produces compilable Rust
- [x] `break`/`continue` produce compilable Rust
- [x] At least 5 compile-path integration tests added
- [x] `cargo build` passes with 0 errors
