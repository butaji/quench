# `quench-node-test`

The test runner executes Node compatibility fixtures through the shared Quench
VM and `quench-node`. It owns discovery, execution, and outcome classification.

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

## Native codecs for Node parity runs

Node 26's `node:zlib` needs the matching native zlib and Brotli bindings. Cargo
enables them when `pkg-config` can find both packages; otherwise it selects
portable fallbacks that do not provide full Node 26 parity. The build tools keep
their strict `tests/node` Git-submodule revision check by default. For an
extracted Node release source tree without `.git`, opt into release-source mode
and name the version expected from `src/node_version.h`:

```sh
node_source=/path/to/node-v26.12.1
codec_root="$PWD/work/node-codecs-v26.12.1"
python3 tools/build-node-zlib.py \
  --node-source "$node_source" --prefix "$codec_root/zlib" \
  --release-source --expected-node-version v26.12.1
python3 tools/build-node-brotli.py \
  --node-source "$node_source" --prefix "$codec_root/brotli" \
  --release-source --expected-node-version v26.12.1
export PKG_CONFIG_PATH="$codec_root/zlib/lib/pkgconfig:$codec_root/brotli/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
pkg-config --modversion quench-node-zlib quench-node-brotli
cargo build --profile iteration --bin run-parallel -p quench-node-test
```

Use separate codec prefixes so each archive retains its own
`provenance.json`. The release-source provenance records the Node version,
version-header hash, codec source hashes, build-definition hashes, compiler,
platform, and archive hash. The scripts do not modify `tests/node` or vendor
codec binaries into the repository.
