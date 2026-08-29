# `quench-node-test`

The test runner executes Node compatibility fixtures through `quench-runtime`
and `quench-node`. It owns discovery, execution, and outcome classification.

## Rules

- Fixtures run as ordinary JavaScript through Quench; the runner never
  reimplements Node's test harness or rewrites fixture behavior.
- Classify outcomes explicitly as pass, fail, skip, crash, or timeout.
- A pass requires the fixture's assertions and required host effects to run;
  an inert stub is not evidence.
- Compare observable results with the local Node oracle, including values,
  errors, exit status, callback order, and externally visible I/O.
- Keep fixture inventory and result records reproducible from a clean checkout.
- Keep Node-specific semantics in `quench-node`; the runtime remains unaware of
  the runner and the Node test suite.
