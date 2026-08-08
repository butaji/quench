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
  lifecycle semantics remain queued.
- Item 6: partially improved. Stage 2339 verifies two concurrent HTTP requests
  with independent response bodies and clean shutdown; upstream multi-request
  failures remain specific to harness/agent interactions.
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
  module keys to realpaths did not change the failure and was reverted; the
  next loader probe must distinguish package-root traversal from swallowed
  nested execution exceptions.
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
  retained.

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

The same stale report's representative dgram entries
(`test-dgram-bind-sync.js`, `test-dgram-bytes-length.js`, and
`test-dgram-bind-error-repeat.js`) also pass individually on the current
runtime. A fresh differential run is required before treating those clusters
as remaining failures.
