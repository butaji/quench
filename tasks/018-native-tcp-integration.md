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

Stage 2316 adds native write shutdown and verifies a real client half-close:
the server receives EOF, runs `end`, and completes cleanup. The upstream
`test-net-allow-half-open-async-iter.js` still fails at its larger harness
callback path, so this stage is only a transport primitive/lifecycle claim.

Stage 2317 makes the host poll phase drain all pending native accepts in one
turn and removes destroyed sockets from its native registration set. Three
concurrent native clients complete independent ping/pong writes in the
focused stage.

Stage 2318 also fixes the general `net.Server.close()` lifecycle: the server
now emits its asynchronous `close` event after closing, including when no
callback is supplied. The authoritative `test-net-server-close.js` passes.

Stage 2331 exposes native client and accepted-server `localAddress`,
`localPort`, `remoteAddress`, and `remotePort` metadata, backed by the actual
TCP handles, including `socket.address()`. The focused native address stage passes; in-memory address
metadata remains intentionally separate.

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
- Stage 2316: gated native half-close and EOF delivery (passing).
- Stage 2317: gated concurrent native client acceptance (passing).
- Stage 2318: `Server.close` event lifecycle (passing; upstream verified).
- Stage 2331: native socket address metadata (passing).
- Next: one accepted socket with one write/read round trip and explicit close.
- Then: half-close exchange from Node's
  `test-net-allow-half-open-async-iter.js`.
- Finally: run the owned `net` queue and refresh the differential report.

The Node suite remains the oracle; LLRT and Deno references remain documented
in `docs/authoritative-test-sources.md`. Until the host poll phase exists, raw
TCP fixtures and applications requiring real network sockets remain unresolved.

## Additional scheduler boundary

The stream and VFS probes expose a second host-loop requirement: the current
`setTimeout`/`setInterval` shim performs delayed sleep synchronously inside a
JavaScript microtask. Consequently, a long-delay timer can run before queued
`setImmediate` or promise work. This is observable in the full stream
backpressure and watch-promises fixtures, while ordinary timer fixtures and
focused stream contracts pass. A future host scheduler step must provide
non-blocking delayed callbacks with Node ordering; changing callback behavior
in JavaScript alone would misrepresent timer semantics.

An isolated clean-worktree prototype replaced synchronous delay with
cooperative deadline polling using `__quench_now_ns()`. Its minimal stream
trace advanced from two reads with no callbacks to four reads with callbacks,
confirming the ordering diagnosis. It still failed the full backpressure and
watch-promises fixtures and consumed CPU during long waits, so the prototype
was discarded rather than integrated. A production fix needs host-backed
non-blocking timers or an equivalent bounded scheduler mechanism.

The local `rquickjs` 0.9 source confirms two host-backed implementation routes:
`AsyncRuntime`/`AsyncContext` with `Promise::wrap_future()` can host a Rust
sleep future, but would require migrating the synchronous harness; alternatively
the existing `Runtime` can keep its shape while a Rust deadline registry is
polled alongside `__quench_io_poll()` and `execute_pending_job()`. The latter
is the smaller integration seam: JavaScript timer creation/cancellation would
register deadlines, and the host loop would dispatch due callbacks without
blocking the JS job queue.
