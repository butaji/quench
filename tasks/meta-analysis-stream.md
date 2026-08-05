# Conformance Workflow

The test262 digest output is the sole conformance SSOT. Do not copy stage counts,
failure counts, completion claims, or active-work notes into documentation.

For a failing stage:

1. Run the stage with `TEST262_DIGEST=1` and group failures by cause.
2. Write one failing Rust unit test for each distinct bug.
3. Make the smallest fix on the canonical spec-op path
   (`eval/ops.rs` + `value/*`). Observable spec algorithms go in the active
   self-hosted JS builtins layer; Rust changes are reserved for canonical
   primitives, storage/native-memory, engine integration, performance,
   crate-backed functionality, or documented lower-LOC direct bindings.
4. Run the unit test, its suite, the stage, formatting, and clippy.
5. Treat the test output as the result. Do not record stage results or
   progress metadata in `tasks/` or `docs/`.
