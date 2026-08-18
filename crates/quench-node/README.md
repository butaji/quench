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
