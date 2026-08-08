# Ten remaining evidence boundaries

This queue turns the remaining failures into concrete evidence contracts. Each
item must have a focused stage, an authoritative upstream fixture or app probe,
and a task-log result before it is considered improved.

1. Direct `net.Socket.connect()` must deliver a distinct server-side socket.
2. Native `net` options and lifecycle events need differential ordering checks.
3. Socket timeout liveness and `ref`/`unref` behavior need a timer trace.
4. Server keep-alive options must reach accepted native/in-memory handles.
5. Raw HTTP parsing over `net` needs request-line/header/body framing.
6. HTTP multi-request sequencing needs response/close ordering evidence.
7. Dgram remaining multi-socket and cluster fixtures need handle identity traces.
8. The full `fs.access()` fixture needs callback-count and credential sequencing.
9. ESLint needs a minimal trace of the RegExp flags/String.replace recursion.
10. Streams need a `common.mustCall` demand-scheduling trace for callbacks.

## Working rule

Do not claim a broad compatibility fix from a narrow probe. For each item,
record the failing upstream fixture, the reduced reproduction, the retained
general behavior, and the remaining difference. Push each verified item as its
own commit.

## Status

- Item 1: improved. Stage 2333 verifies explicit `Socket.connect()` server
  delivery; upstream `test-net-socket-tos.js` passes. The local-address
  reconnect fixture still has a separate close/callback mismatch.
- Item 2: partially improved. Stage 2334 verifies `connecting` true before
  the connect turn and false during the connect event; remote-address fixture
  lifecycle callbacks remain unresolved.
- Item 3: improved. Stage 2335 and upstream
  `test-net-socket-timeout-unref.js` pass with socket `ref`, `unref`, and
  `hasRef` state.
- Item 4: partially improved. Stages 2336–2337 apply connection and server
  `keepAlive`/`keepAliveInitialDelay` options to socket handles; the larger
  client/server keep-alive fixtures still have callback/lifecycle gaps.
- Item 5: partially improved. Stage 2338 verifies terminal write errors after
  a non-half-open peer EOF; stage 2347 bridges raw in-memory `net` connections
  to listening HTTP servers and verifies the server-side connection event.
  Stage 2348 adds a raw HTTP/1.1 GET round trip with request headers and
  serialized response headers/body. Stage 2349 adds two pipelined keep-alive
  requests on one socket. Stage 2350 verifies Content-Length request-body
  buffering and data/end delivery; stage 2351 verifies client half-close
  behavior. Stage 2352 adds chunked request-body decoding, and stage 2353 adds
  standards-compliant chunked response framing. More complete HTTP parsing and
  lifecycle semantics remain queued. Stage 2365 consumes chunked request
  trailers, exposes them on `IncomingMessage.trailers`, and preserves the next
  keep-alive request. Stage 2366 adds `ServerResponse.addTrailers()` and
  serializes response trailers in the terminating chunk.
- Item 6: partially improved. Stage 2339 verifies two concurrent HTTP requests
  with independent response bodies and clean shutdown; upstream multi-request
  failures remain specific to harness/agent interactions.

The current `test-http-client-abort-keep-alive-queued-tcp-socket.js` probe
reaches the public custom-agent path but receives quench's intentional
`ENOTSUP` from `NodeHttpAgent.createConnection()` where Node requires a live
reusable socket (and reports `ECONNRESET` only for `destroy`, not `abort`).
This is now classified as the remaining agent/transport integration boundary;
a prototype custom socket path reached the queued-request scheduler but sent
a response to the request marked `mustNotCall`, so it was reverted. No
error-code-only shim was added. A first attempt to cancel queued requests by
releasing their waiter and active slot reduced the max-sockets fixture from
its baseline to four of six callbacks, so it was also reverted; cancellation
ownership still needs a separate state model.

- Item 7: partially improved. Stage 2340 verifies default-address bind,
  implicit sender bind, packet delivery, remote metadata, and close callback
  ordering. Stage 2341 generalizes multicast-interface rejection to the full
  IPv4 multicast range while preserving `0.0.0.0` as the default-interface
  selection. The remaining exclusive implicit-bind fixture still depends on
  unsupported cluster worker behavior; the IPv6 multicast cases need a
  platform-backed validation seam.
- Item 8: partially improved. Existing stages 2309 and 2311 verify synchronous
  and callback/promise permission errors, including single-callback ordering.
  The authoritative fixture cannot currently provide stronger evidence in this
  harness because its root-user guard and `process.setuid('nobody')` branch
  terminate with an opaque QuickJS exception; credential-switch behavior must
  be fixed before claiming the full fixture.
- Item 9: partially improved. Stage 2342 covers `node:` builtin normalization
  through the central dispatcher. Replacing loader-time regex stripping with
  `startsWith()`/`slice()` removes the previous native
  `RegExp.prototype.flags`/`Symbol.replace` recursion while loading ESLint.
  ESLint now advances to a separate package-resolution failure for
  `prelude-ls`, so the full application gate remains unresolved. Direct loads
  of `prelude-ls`, `type-check`, `optionator`, and the individual ESLint
  dependencies pass; loading the complete `eslint` graph first is the
  differentiating sequence, indicating a nested loader-cache transition rather
  than a missing installed package.
  Additional tracing confirms the package root is discovered; a `RangeError`
  occurs while resolving or executing `prelude-ls/lib/index.js` from nested
  `levn`/`type-check` loads, then gets reported as `MODULE_NOT_FOUND`. Direct
  loading succeeds, narrowing this to nested local-module cache/execution
  re-entry. A focused test of normalizing nested `require.cache` and child
  module keys to realpaths did not change the failure and was reverted. A
  sharper probe confirms all dependency loads pass and the failure begins at
  `new Linter()`, where native `RegExp.prototype.flags`/`Symbol.replace`
  recursion occurs; standalone equivalent regex flag cases pass. The next
  fix must target that runtime interaction rather than package resolution.
  Temporarily bypassing quench's `Object.setPrototypeOf` wrapper changes the
  symptom back to a masked `prelude-ls` resolution error but still does not
  construct `Linter`, so that probe was reverted and no prototype behavior was
  changed.
- Item 10: partially improved. Stage 2343 reproduces the upstream
  `common.mustCall` readable backpressure shape and now verifies all four
  `_read()` demands and three writable callbacks. The fix removes a stale
  `reading = true` assignment that suppressed demand after `push()` had
  already completed. Broader concurrent/infinite-stream cadence fixtures still
  need verification. Stage 2344 traces the remaining upstream difference:
  quench reaches all writable callbacks but invokes the batched `_read()` hook
  three times where Node invokes it eleven times, so readable buffer-demand
  accounting remains the next stream implementation target. A fresh
  authoritative rerun of `test-stream-backpressure.js` still fails at the
  shared `mustCall` contract (`expected 11 calls, got 1`); an attempted
  read-ahead correction was reverted after it overfilled the buffer before
  the pipe's first write callback, so no unverified scheduling behavior was
  retained. A focused `_final()` double-callback trace also confirms `finish`
  fires once while the duplicate error remains suppressed (`finish=1,
errors=0`); a direct finish-guard change was reverted pending an
  event-loop/auto-destroy trace.
  Stage 2370 isolates the remaining constructor boundary: the post-bootstrap
  callable-constructor adapter discarded `new.target`, so
  `class TestWritable extends stream.Writable` instances lost `_write()` and
  `_final()` methods and had the base `NodeWritable` prototype. The adapter
  now uses `Reflect.construct(Constructor, args, new.target || Constructor)`;
  the focused subclass regression passes. The upstream duplicate-callback
  fixture remains to be rerun through the harness after its path/runner
  invocation is normalized. The runner probe found that positional fixture
  execution exits silently, while `--test-dir <fixture>` reports the result;
  `tools/run-node-tests.sh` now uses the explicit mode for single files.
  A fresh stream differential run now passes
  `test-stream-err-multiple-callback-construction.js` and
  `test-stream-catch-rejections.js`. It still fails
  `test-stream-drop-take.js` (`Callback 4` is never observed) and
  `test-stream-backpressure.js` (`_read()` expected 11 calls, got 1), proving
  that iterator cancellation and byte-buffer demand are separate remaining
  contracts. Stage 2372 independently passes finite, infinite, chained, and
  abort-signal `drop()`/`take()` results, so the upstream callback gap is
  narrowed to the fixture's combined promise/finally scheduling rather than
  basic slice output.

## New fs evidence

The temporary stage-2345 callback-label probe showed that callback-style
`fs.access()` completes, while `fs.promises.access()` reactions are not
observed before the following timer turn. This explains the full fixture's
missing callback and moves the next fs fix into promise-job/event-loop
integration; no filesystem permission behavior was changed.

Stage 2346 now verifies callback-before-promise ordering for successful
`fs.access()` calls. `fs.promises.access()` uses a direct async sync-check so
its reaction is not delayed behind the callback wrapper. The full upstream
fixture still has a separate invalid/error callback-count mismatch.

The stale differential report's top `fs.cp` cluster was rechecked against the
current runtime: `test-fs-cp-async-skip-validation-when-filtered.mjs`,
`test-fs-cp-promises-async-error.mjs`, and
`test-fs-cp-sync-async-filter-error.mjs` all pass individually. They should be
removed from the next differential queue refresh; this is evidence correction,
not a new filesystem implementation claim.

The current upstream `test-http-server.js` still exits with zero dispatched
requests. A transport probe showed that its client writes from the `connect`
callback before the in-memory peer is attached; naïve pending-write flushing
regressed the verified raw HTTP keep-alive/response lifecycle and was reverted.
The remaining fix must coordinate pending writes with HTTP response socket
assignment before claiming the upstream fixture. Reordering peer attachment
before the client `connect` event causes the same regression, so that approach
was also reverted; the next implementation needs an explicit parser-ready
queue boundary. The raw response path also lacked `_httpMessage` cleanup;
clearing that field after a response completes is safe across stages, but it
does not by itself resolve the pre-peer write race. Allowing reuse of a
completed response in `assignSocket()` and reordering peer attachment were
tested together; they still failed during response property initialization,
so both experiments were reverted.

Stage 2362 independently verifies that callback-style and promise-style
`fs.access()` each deliver exactly once for a missing path, including the
`ENOENT` error code. This narrows the unresolved upstream `test-fs-access.js`
callback-count failure to its combined credential, invalid-argument, and
promise sequencing rather than the basic missing-path error path.

Stages 2363 and 2364 additionally verify the upstream promise rejection stack
shape (`at async Object.access`) and the non-root read-only `W_OK` `EACCES`
behavior. Both pass, so neither the promise stack contract nor the basic
permission decision explains the remaining aggregate-fixture mismatch.

Current application-gate refresh: stages 2047, 2069, 2080, 2081, 2104, and
the repository smoke app all pass. The current upstream
`test-http-agent-maxsockets-respected.js` baseline remains `Callback 1:
expected 1 calls, got 0`, confirming that the unresolved agent issue persists
after the reverted cancellation experiments.

The same stale report's representative dgram entries
(`test-dgram-bind-sync.js`, `test-dgram-bytes-length.js`, and
`test-dgram-bind-error-repeat.js`) also pass individually on the current
runtime. A fresh differential run is required before treating those clusters
as remaining failures.
