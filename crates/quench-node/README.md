# `quench-node`

`quench-node` is the Node.js compatibility host built on
[`quench-runtime`](../quench-runtime).

## Boundary rules

- This crate owns Node module registration, host capabilities, process
  boundaries, and Node-compatible errors and effects.
- The runtime owns JavaScript syntax and semantics and must not depend on this
  crate.
- The host must never detect benchmark names or source, scores, checksums, or
  another JavaScript engine.
- Every script follows the same host path and preserves Node-observable values,
  ordering, errors, exit status, and I/O.
