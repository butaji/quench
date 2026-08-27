# `quench-test262` rules

- Execute each test with the declared test262 harness and through the host
  contract; never rewrite, replace, or shim harness behavior.
- Keep discovery order deterministic and make per-test outcomes independent of
  isolation, sequential execution, or worker scheduling.
- Reset globals, environments, private state, module state, and microtasks at
  every test boundary.
- Classify every test outcome explicitly. Do not infer passes from missing
  records or hide timeouts and crashes.
- Keep metadata, harness composition, module graphs, and runner contracts in
  this crate; keep JavaScript semantics in `quench-runtime`.
- Treat determinism checks as invariants, not as progress totals or stale
  coverage claims.
