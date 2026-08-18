# `quench-node-test`

Test runner for the [`quench-node`](../quench-node) host. Owns:

- the upstream Node fixture submodule at `node-tests/`
  (`https://github.com/nodejs/node`, path `test/`);
- the runner that discovers, composes, and executes those
  fixtures through the host contract;
- the completion classifier that maps a host run to pass / fail /
  skip / crash.

**Boundary:** this crate never modifies the upstream fixture
tree, never shims or rewrites Node harness behavior, and never
depends on the Node API surface in a way that influences fixture
outcomes. The host (`quench-node`) is forbidden from knowing
about this crate, the runner, the fixtures, or Node test policy.
See [`../../docs/adr/0002-quench-node-scope.md`](../../docs/adr/0002-quench-node-scope.md).
