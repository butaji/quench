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

## Throughput strategy

Reduce time from failure discovery to a verified canonical fix while keeping
the digest as the sole conformance SSOT.

1. Run `TEST262_QUICK=1` to discover representative failure groups.
2. Write one unit-test reproducer per distinct root cause and fix the canonical
   spec-op or builtin path.
3. Run the complete stage digest, then formatting, clippy, and the relevant
   unit suite.

Prioritize expected failures cleared per hour. Shared operations such as
`ToPrimitive`, property operations, descriptors, iterators, callability, and
equality outrank local builtin symptoms when one defect can affect many stages.

When extending diagnostics, group by stable root-cause fields such as phase,
error type, runtime location, execution mode, and normalized message. Measure
discovery, harness loading, context/bootstrap, parsing, execution, and cleanup
before optimizing them.

Independent stages may run concurrently only with isolated result files and a
serialized final advancement step. Helper scripts must not create a second
progress counter.
