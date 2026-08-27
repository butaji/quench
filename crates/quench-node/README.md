# `quench-node`

Node.js-API compatibility host built on top of
[`quench-runtime`](../quench-runtime). The runtime is a pure
JavaScript engine; this crate is the only piece of the workspace
allowed to know what "Node" is.

**Scope, v1 module set, and acceptance bar:** see
[`../../docs/adr/0002-quench-node-scope.md`](../../docs/adr/0002-quench-node-scope.md).

**Ordered plan and stages:** see
[`../../docs/NODE-STAGES.md`](../../docs/NODE-STAGES.md).

**Boundary:** the runtime never learns about this crate. The
fixtures / runner / classifier live in
[`../quench-node-test`](../quench-node-test). This crate knows
about Node; it knows nothing about the Node test runner.

## CLI

The `quench-node` binary runs a script through the host like a Node
binary, with genuine `process.argv` / `process.execPath` and exit
code semantics. This is the entry point real npm apps launch under:

**This crate is Node compatibility only. It contains no benchmark dispatch,
fixture detection, score/checksum logic, or delegation to another JavaScript
engine. Benchmark suites must execute through the same host path as every
other script.**

```sh
cargo run -p quench-node -- <script.js> [args...]
# or, from a built tree:
target/debug/quench-node <script.js> [args...]
```

Internals: the harness lives in `src/run.rs`
(`quench_node::run::run_script`) — the single canonical owner of the
`node <script>` pipeline (install host, run script as CJS, pump the
event loop, run `exit` handlers, resolve the exit code).
