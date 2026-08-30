# Repository rules

Quench implements Node-compatible APIs on a JavaScript engine. Treat API
shape, validation, errors, calling conventions, exports, and evidence as data.
Generate repetitive registration and wrappers from those facts; handwrite only
irreducible observable behavior. Keep the Rust host minimal and do not add a
separate runtime crate.

## Architecture principles

1. Represent each semantic fact once.
2. OXC owns syntax; Quench does not create another syntax tree.
3. Keep static structure as data or eliminate it before runtime.
4. Let VM code represent only dynamic uncertainty.
5. Semantic abstractions do not imply runtime allocations.
6. Share semantic mechanisms while specializing physical execution.
7. Generate mechanical consequences from one declaration.
8. Give no subsystem its own universe unless its semantics require one.
9. Treat types and profiles as facts, not as another runtime or optimizer.
10. Treat facts as `Proven`, `Guarded`, or `Unknown`.
11. Complete slow semantics and cheap `Unknown` behavior precede guarded fast paths.
12. Never optimize through observable JavaScript behavior.
13. Keep heap references compact and account for generated code, static data,
    caches, and native code in the complexity budget.
14. Optional native execution consumes the same residual operations, remains
    bounded and disposable, and owns no alternative semantics.
15. If something can disappear before runtime, justify why it exists.

## Compatibility and benchmark rules

- `quench-node` owns only the Node host/API boundary; `quench-runtime` owns
  JavaScript semantics.
- Verify Node behavior against the local Node oracle and upstream source.
- Compare observable values, descriptors, identity, ordering, errors, exit
  status, and host effects.
- Benchmark code is measurement only. Production code must not detect fixture
  names or source, scores, checksums, suite markers, or another engine.
- Every optimization must be reusable outside its originating workload,
  guarded by facts, and fall back to complete ordinary semantics.
- Do not add benchmark dispatch, benchmark build hooks, CI configuration, or
  unrelated external-project changes.
- Keep declarations explicit; generated wrappers may replace duplicated
  mechanics but may not obscure exceptional behavior.
