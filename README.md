# quench-node

Node-compatible JavaScript runtime built in Rust on top of
[rquickjs](https://github.com/DelSkayn/rquickjs), with readable JavaScript
polyfills and a staged compatibility harness.

## Quick start

```sh
cargo run -p quench-node -- --stage 284
tools/run-node-tests.sh tests/node/test/parallel/test-querystring.js
tools/compat-coverage.sh
```

The Node test suite is tracked as the `tests/node` submodule. Compatibility
stages live under `tests/node-compat`; each stage is committed and verified
before advancing.

## Scope

The repository contains only the `quench-node` crate, its polyfills, the Node
test submodule, compatibility stages, and the small harness needed to run
them. Polyfills are intentionally kept readable and uncompressed.

`tools/compat-coverage.sh` reports the current fixture and upstream-test
inventory. It deliberately reports Node API coverage as `unmeasured`: a count
of focused fixtures is not a valid percentage of the full Node API surface.

## Runtime boundary

`quench-node` uses `rquickjs` as its JavaScript engine and Rust host boundary.
There is no `quench-runtime` crate in this repository, and compatibility work
must not add or restore one. Keep engine integration in the `quench-node` crate
and API behavior in the JavaScript polyfills.

## License

MIT
