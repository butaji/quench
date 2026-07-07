> **P0 build blocker.** The runtime cannot be compiled or tested until this is fixed.

# Task 330: Restore green build for quench-runtime

## Status: COMPLETED

## Summary

`cargo check -p quench-runtime`, `cargo build -p quench-runtime`, and `cargo test -p quench-runtime` now succeed. The build is green.

## What changed

The ~44 errors originally observed in `crates/quench-runtime/src/interpreter/mod.rs` (missing `Control`, `assign_to_simple`, hoisting helpers, and non-exhaustive patterns for `ClassDeclaration`, `SuperMember`, `SuperCall`, `ClassExpression`, `UnaryOp::Plus`, `UnaryOp::Delete`) were resolved as part of the interpreter refactor and TDZ work.

## Acceptance criteria

- [x] `cargo check -p quench-runtime` exits successfully.
- [x] `cargo build -p quench-runtime` exits successfully.
- [x] `cargo test -p quench-runtime` compiles and runs.

## Verification

```bash
cargo check -p quench-runtime
cargo build -p quench-runtime
cargo test -p quench-runtime
```

## Notes

- Three warnings remain in `quench-runtime` (unused `Getter`, `Setter`, and `set_this_binding`). These are not build blockers and are tracked by normal cleanup work.
- Remaining interpreter/module-split cleanup is tracked in Task 256.
