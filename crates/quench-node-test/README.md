# `quench-node-test`

Test runner for the [`quench-node`](../quench-node) host. Owns:

- the upstream Node fixture submodule at `node-tests/`
  (`https://github.com/nodejs/node`, path `test/`);
- the runner that discovers, composes, and executes those
  fixtures through the host contract;
- the completion classifier that maps a host run to pass / fail /
  skip / crash.

## Boundaries

The runner never:

- re-implements Node's test runner, harness, or assertion library;
- shims or rewrites Node's `common` test helper;
- depends on the Node API surface in a way that influences
  fixture outcomes.

It is a pure-JS-execution pipeline: each fixture is parsed
and reduced by `quench-runtime` and executed by `quench-node`.

## Usage

Run the full compat suite:

```sh
cargo run -p quench-node-test --bin run-compat
```

Enumerate the suite:

```sh
cargo run -p quench-node-test --bin run-compat -- --list
```

Subset by name substring:

```sh
cargo run -p quench-node-test --bin run-compat -- --filter os
```

Run a single ad-hoc script through the host:

```sh
cargo run -p quench-node-test --bin run -- crates/quench-node-test/node-tests/test-os.js
```

## Suite

The compat suite is a plain directory of Node compat API
scripts under `node-tests/` (currently 28 scripts). Each test
is a self-contained file:
it requires the relevant `node:` module, runs a small set of
operations, and throws on failure. There is no `node:test`
runner, no `common.mustCall`, and no harness. The host must
satisfy each script's observable behavior.

Current coverage (28 scripts):

- `test-assert.js` — assert.ok function shape
- `test-buffer.js` — Buffer.from / Buffer.alloc / Buffer.concat
- `test-child-process.js` — child_process.spawnSync (real subprocess
  execution, status/stdout/error codes)
- `test-console.js` — console.log/info/warn/error/debug/trace
- `test-dns.js` — dns.lookup (callback)
- `test-events.js` — events.EventEmitter round-trip
- `test-fs.js` — fs.readFileSync / readdirSync
- `test-modules.js` — every v1 `node:` module resolves
- `test-http.js` — real HTTP server (createServer/listen) answered by a
  raw net client request, asserting the parsed req and serialized res
- `test-net.js` — net.isIP family + real loopback TCP (server.listen,
  socket write/read echo, close) through the event-loop pump
- `test-os.js` — os.cpus / totalmem / freemem / networkInterfaces
- `test-path.js` — path.join / normalize / dirname / basename /
  extname / isAbsolute / relative + posix/win32 subnamespaces
- `test-process.js` — process.version / platform / arch / pid
- `test-querystring.js` — querystring.parse / stringify / escape
- `test-stream.js` — stream module shape (Readable/Writable/...)
- `test-util.js` — util.format / util.inspect shape
- `test-vm.js` — vm + string_decoder shape
- `test-dgram.js` — dgram shape (createSocket stub)
- `test-https.js` — https shape (request / get stubs)
- `test-zlib.js` — zlib + perf_hooks shape
- `test-tls.js` — tls + cluster shape
- `test-inspector.js` — inspector + trace_events shape
- `test-repl.js` — repl + wasi shape
- `test-worker_threads.js` — worker_threads shape (Worker stub)
- `test-sea.js` — sea shape (isSea stub)
- `test-test.js` — node:test shape (test stub)
- `test-stream-web.js` — stream/web + stream/consumers shape
- `test-timers.js` — setTimeout / setImmediate / setInterval
- `test-tty.js` — tty.isatty
- `test-url.js` — url.parse / format
