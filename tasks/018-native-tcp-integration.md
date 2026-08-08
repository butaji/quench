# Native TCP integration plan

The verified Stage 2312 host primitives provide nonblocking TCP bind, connect,
accept, read, write, and close operations. They are intentionally not yet wired
into the public `net` module because the current host loop only drains
JavaScript jobs; it does not own a native-I/O wait/poll phase.

## Required integration contract

1. The host must expose a bounded native poll step that can report readable,
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
- Next: one accepted socket with one write/read round trip and explicit close.
- Then: half-close exchange from Node's
  `test-net-allow-half-open-async-iter.js`.
- Finally: run the owned `net` queue and refresh the differential report.

The Node suite remains the oracle; LLRT and Deno references remain documented
in `docs/authoritative-test-sources.md`. Until the host poll phase exists, raw
TCP fixtures and applications requiring real network sockets remain unresolved.
