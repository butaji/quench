# Native TCP integration plan

The verified Stage 2312 host primitives provide nonblocking TCP bind, connect,
accept, read, write, and close operations. They are intentionally not yet wired
into the public `net` module because the current host loop only drains
JavaScript jobs; it does not own a native-I/O wait/poll phase.

Stage 2313 now provides the first host-owned scheduling seam: the host invokes
`globalThis.__quench_io_poll()` before draining pending jobs and on each
`beforeExit` turn. The stage verifies that this hook is visible to JavaScript,
but it intentionally does not claim native readiness delivery or public
`net` integration yet.

Stage 2314 adds `__quench_tcp_readable()`, a non-destructive native stream
readiness probe with distinct would-block, readable, and EOF results. Its
loopback stage passes after a real write and confirms the signal is available
before consuming data.

Stage 2315 wires the first public `net` loopback path to those primitives when
the internal `__quenchNativeTransport` option is present. `Server.listen()`
binds a real listener and reports its ephemeral port; `createConnection()`
creates a real client; the host poll hook delivers `connection` and `data`.
The focused ping/pong stage passes. The option is intentionally gated while
ordering, half-close, liveness, and ordinary Node option normalization are
completed.

## Required integration contract

1. The host must extend the Stage 2313 hook into a bounded native poll step that can report readable,
   writable, EOF, and error states without an unbounded JavaScript interval.
2. `net.Server.listen()` must register a native listener, publish its actual
   ephemeral port, and deliver accepted sockets through the existing EventEmitter
   surface.
3. `net.Socket` must preserve Node ordering for `connect`, `data`, `end`,
   `finish`, `close`, and error events, including `allowHalfOpen`.
4. `destroy()`, `end()`, `unref()`, and server close must release native handles
   and remove the runtime liveness reference exactly once.
5. Each step needs a focused stage before enabling it for upstream `net` tests.

## Verification order

- Stage 2312: host primitive loopback exchange (passing).
- Stage 2313: host-owned I/O poll scheduling seam (passing).
- Stage 2314: native stream readiness probe (passing).
- Stage 2315: gated public `net` TCP ping/pong loopback (passing).
- Next: one accepted socket with one write/read round trip and explicit close.
- Then: half-close exchange from Node's
  `test-net-allow-half-open-async-iter.js`.
- Finally: run the owned `net` queue and refresh the differential report.

The Node suite remains the oracle; LLRT and Deno references remain documented
in `docs/authoritative-test-sources.md`. Until the host poll phase exists, raw
TCP fixtures and applications requiring real network sockets remain unresolved.
