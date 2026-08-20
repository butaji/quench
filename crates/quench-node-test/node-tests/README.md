# Node compat API tests for quench-node

Hand-curated, minimal compat tests that exercise the Node API
surface implemented by `quench-node`. Each test is a self-contained
script: it requires the relevant `node:` module, runs a small set
of operations, and exits with status 0 on success, non-zero on
failure. The `quench-node-test` runner classifies each script as
`Pass` / `Fail` / `Skip` based on the exit code and any thrown
error. No `node:test` runner, no `common.mustCall`, no harness.

This is a curated subset — see `docs/adr/0002-quench-node-scope.md`
for the full v1 module set. The tests below are the ones the host
can run end-to-end today.
