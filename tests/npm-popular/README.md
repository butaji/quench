# Popular npm package smoke fixtures

The shortlist uses npm's public weekly download counts for 2026-09-28 through
2026-10-04, recorded in [`priorities.json`](priorities.json). It starts with
high-download utilities that exercise JavaScript semantics and common Node
package loading, followed by HTTP client and framework scenarios. The pinned
versions are in `package-lock.json`.

Install packages outside the fixture subtree so the Node fixture runner does
not mistake package source files for test cases. The install helper refuses to
overwrite an existing `tests/node_modules` directory:

```sh
tests/npm-popular/install.sh
```

Then compare against Node and run the same fixtures through Quench:

```sh
for fixture in tests/npm-popular/test-*.cjs; do
  node "$fixture"
done
cargo run --profile iteration -p quench-node-test --bin run-compat -- tests/npm-popular
```

`tests/node_modules` is local install state and is ignored by Git. The existing
Express, Koa and Fastify scenarios remain under `tests/frameworks`.
