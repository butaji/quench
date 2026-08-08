# Current compatibility push — Node 24 application compatibility and auditable baselines

This progress log follows the Node 24 application-runtime contract: upstream
Node tests are the primary oracle, focused stages are regression guards, and
Hono plus a representative CLI are release-facing application gates.
The authoritative Node, LLRT, Deno, WPT, and Test262 references are maintained
in `docs/authoritative-test-sources.md`.

## Verified progress

- Focused contracts: **1,945/1,945 passing**.
- Live inventory: 58 canonical modules, 57 statically registered, one
  platform-limited runtime omission (`node:sea`), and 186 observed Node globals.
- Latest completed differential: 4,682 fixtures with 922 exact matches, 2,461
  Quench-only failures, 537 output mismatches, 87 timeouts, and 190 fixtures
  explicitly classified as environment-limited.
- `stream/iter` now covers broadcast cancellation/abort propagation,
  `fromWritable()`, and preservation of typed-array chunks in `array()` and
  `arraySync()`.

## Current verification

- The fresh full differential completed against canonical `main`: 4,682
  fixtures, 922 exact matches, 1,399 both-failed, 2,461 Quench-only failures,
  537 output mismatches, 180 Node-only failures, 87 timeouts, and 190 explicitly
  environment-limited fixtures. No worker failed.
- Deno formatting, `cargo build -p quench-node`, and `git diff --check` pass.

## Next queue

- Refresh the decision report from the completed differential.
- Continue the owned `streams-events-async` queue, using isolated upstream
  fixtures before changing shared stream semantics.
- The current top owned queue is HTTP (56 callback failures); raw TCP framing
  cases are classified individually when their missing host transport is the
  actual cause.
- Preserve explicit platform classifications for native TLS, HTTPS, HTTP/2,
  inspector, QUIC, and other host-integrated APIs.

## Latest slice

- Stage 2034 locks the currently implemented HTTP response header surface:
  `getHeaders()`, `getHeaderNames()`, `flushHeaders()`, and
  `writeEarlyHints()`, plus request `flushHeaders()`.
- The stage passes locally; it is a focused regression guard while the broader
  HTTP callback and agent queue remains open.
- Stage 2035 adds the `http.Server.listen({ port, host }, callback)` contract,
  which was previously accepted but ignored its options object.
- Stage 2036 fixes automatic client `content-length` headers for empty POST and
  PUT requests by normalizing headers before constructing the server request.
  The focused contract now passes for GET, HEAD, DELETE, OPTIONS, POST, PUT,
  and TRACE.
- Stage 2037 adds the public `http.ClientRequest` constructor defaults for
  empty `method` and `path` options.
- Stage 2038 adds `ClientRequest` header introspection via `getHeader`,
  `getHeaders`, `getHeaderNames`, and `hasHeader`.
- Stage 2039 adds chainable `ClientRequest` socket-control methods:
  `setNoDelay`, `setSocketKeepAlive`, and `setSocketTimeout`.
- Stage 2040 adds chainable `ClientRequest.cork()` and `uncork()` buffering
  controls with balanced nesting behavior.

Stages 2034–2040 all pass together. The upstream listener-leak fixture remains
host-transport-specific: it requires native socket creation and keep-alive
reuse, while this runtime intentionally uses the in-memory HTTP transport.

The authoritative serial focused gate now passes **1,956/1,956** stages with
zero failures. The gate generated repository-root fixture artifacts during its
run; those artifacts were removed after verification, leaving the worktree
clean.

The fresh focused gate now passes **1,959/1,959** stages with zero failures.
This baseline includes stages 2045–2047, the CommonJS package-loader fixes,
and the ESM `fs/promises` named-export fix. It also corrected stale focused
stage assumptions in stages 1747 and 1803. The run completed serially in 340
seconds with zero retries.

The next package-loading slice is stage 2041: the ESM resolver now searches
ancestor `node_modules` directories and reads package `exports`, `module`, and
`main` entries. This is the missing resolution layer exposed by the Hono
example.
The package loader now also honors the nearest package `type: "module"` for
`.js` files, allowing ESM package graphs such as Hono’s to load correctly.
The Hono module itself now loads under `quench-node`; its asynchronous
`app.fetch()` result still requires the runtime’s pending-Promise/microtask
drain support before a standalone script can print the response.
Stage 2042 verifies that basic top-level `await` itself already passes; the
remaining Hono behavior is an untracked application Promise at process exit.
The ESM entry boundary now finishes the module evaluation Promise. Re-running
the Hono smoke app reaches its awaited `app.fetch()` and reports a real
QuickJS exception instead of exiting after `loaded`; Hono’s async fetch path
remains an open live-application compatibility gap.
The concrete first gap was the missing global `Request`; stage 2043 adds
minimal Web `Request`/`Response` behavior used by Hono-style handlers.
Stage 2044 adds `Request.text()` and `Request.json()` for body-consuming
application handlers.
Live Hono comparisons now match Node for both `GET /` (`200: Hono!`) and
`POST /` JSON (`200: {"doubled":14}`), including the JSON content-type header.

## Latest verified slice

Stage 2045 preserves HTTP client request write boundaries and implements
response `setEncoding()` conversion for in-memory HTTP responses. The focused
stage passes, and upstream `test-http-client-upload.js` now matches Node for
separate `1\\n`, `2\\n`, and `3\\n` request chunks plus the decoded `hello\\n`
response. This advances the Node 24 application-runtime HTTP gate without
adding a native transport dependency.

Stage 2046 aligns console output with Node's format specifiers, including `%s`
and `%j`. This removes formatting noise from upstream HTTP diagnostics and
improves real application logging compatibility.

Stage 2047 adds CommonJS package resolution for ancestor `node_modules`
directories, including package `main`/`module`/`exports` entry selection and
relative dependency loading. A real `ajv` npm application probe now passes
under both Node and quench-node, covering package loading, schema compilation,
validation, and diagnostic errors.
The same slice also restores `module._resolveLookupPaths()` relative-path
classification and Node-style filenames in invalid-JSON `require()` errors;
upstream `test-module-relative-lookup.js` and `test-require-json.js` pass.

The real npm `ajv` application gate was re-run after the fresh differential
baseline and still passes, confirming that the current loader changes remain
usable for a dependency graph outside the focused upstream fixtures.

An attempted second real npm application gate using ESLint exposed two loader
improvements: conditional package `exports` objects and `.cjs`/`.mjs` entry
probes are now supported. ESLint progresses past package resolution but then
hits a separate recursive module/runtime incompatibility, so no passing stage
is claimed; the existing `ajv` gate remains green.

The default-agent close fixture exposed a runtime liveness gap: unref'ed timers
were still firing after the last referenced HTTP server closed. Added shared
referenced-handle accounting for timers and HTTP servers, plus focused stage
2063 for the Node liveness rule.

A follow-up refcount probe could not complete the full client/server close
sequence, confirming that the remaining issue includes HTTP response/request
shutdown ordering rather than only the timer counter. The probe was removed;
stage 2063 remains the passing narrow liveness contract.

HTTP server shutdown now associates in-memory response sockets with their
server port and removes matching pooled default-agent sockets. Focused stage
2065 covers this server-close/agent-pool contract.

Refcount observation showed the unref timer was sleeping synchronously before
the HTTP close microtasks could run. Unref timers now yield while referenced
handles exist, with focused stage 2067 covering the timer/HTTP close ordering.
The upstream `test-http-client-close-with-default-agent.js` now passes as
well; the diagnostic observation stage was removed after the fix.

Re-running the adjacent HTTP abort fixture shows its second normal-response
state transition still reports `aborted === true`; the timer fix does not mask
that independent response-lifecycle bug. The chunk-extension-limit fixture
remains transport/parser-specific and still returns an empty response instead
of Node's 413 framing.

Latest grouped focused verification for stages 2058–2067 (six stage
directories) passes 6/6 with zero retries and zero failures.

After the timer and server-close changes, the complete focused gate now passes
1,974/1,974 stages with zero failures and zero retries in 317 seconds of serial
execution. This is the current focused compatibility baseline.

## Fresh upstream differential

The complete `test/parallel` differential processed all **4,682** fixtures with
zero failed workers. It recorded 898 exact matches, 2,492 quench-only
failures, 529 output mismatches, 502 both-failed fixtures, 174 Node-only
failures, 87 timeouts, and 190 explicitly environment-limited fixtures.

The first actionable filesystem queue item was `fs.cp` mode validation. The
runtime now matches Node's numeric range and integer/type error contract, and
upstream `test-fs-cp-async-invalid-mode-range.mjs` passes.

Stage 2048 fixes the top HTTP abort cluster: destroying an in-memory server
response now delivers client `aborted`, `ECONNRESET` error, and `close` events
in Node-compatible order. The focused stage and upstream
`test-http-abort-client.js` pass. HTTP agent timeout and uninitialized-handle
fixtures remain separate failures.

Stage 2049 adds the client socket `free` event when a keep-alive response is
returned to an agent pool. The focused socket-reuse gate passes; the broader
upstream agent-timeout fixture now reaches a separate timeout/reuse ordering
case that remains queued.

Stage 2050 makes public `Agent.addRequest()` consume manually seeded free
sockets with partial `_handle` objects and complete a direct `ClientRequest`
without invoking unsupported native socket internals. The focused gate and
upstream `test-http-agent-uninitialized-with-handle.js` pass.

Stage 2051 fixes keep-alive reuse ordering by returning the socket to the
agent pool before emitting its public `free` event. A focused second-request
reuse gate now passes; this isolates the remaining upstream timeout fixture to
its timer/custom-agent branch.

Stages 2052 and 2053 add focused coverage for custom Agent socket timeouts and
destroyed-socket replacement. The in-memory socket now exposes Node-compatible
`destroy`, `ref`, `unref`, and `setKeepAlive` methods. Both focused contracts
pass; the combined upstream timeout fixture still hangs in its four-block
lifecycle and remains unresolved.
### 2026-08-08 HTTP information/header follow-up

- Added `response.writeInformation()` to the shared HTTP response surface. It
  emits Node-shaped `information` metadata, including status text and
  `rawHeaders`, for interim 1xx responses.
- Added focused stages 2054 and 2055 for informational responses and
  `ClientRequest#flushHeaders()`; both pass.
- The upstream informational fixture now completes successfully. The
  upstream flush-header fixture still reaches its handler but then exposes an
  asynchronous emitter exception during shutdown; the focused contract stage
  passes, so this is tracked as harness/lifecycle parity rather than claimed
  as an upstream pass.
- Extended `ClientRequest#end()` to accept the Node callback overload. The
  timeout-event fixture still exposes an event-loop/emitter mismatch during
  destruction, so it remains queued rather than being counted as a passing
  compatibility claim.
- Hardened the shared event emitter against stale/non-callable listener slots
  during HTTP shutdown and added stage 2057 for the flush-header shutdown path.
  The focused stage passes; the upstream fixture still reports a separate
  generated-harness `callback.call` exception after the handler completes.
- Switched deferred HTTP server callbacks to `Reflect.apply`, preserving the
  Node callback receiver without depending on a callback object's `.call`
  property. Re-run the upstream fixtures before counting this as resolved.
- Applied the same callback invocation primitive to the core event emitter,
  including error-monitor listeners, to eliminate the remaining foreign
  runtime `.call` assumption in event delivery.
- Fresh focused-gate result: 1,967 of 1,968 stages passed. Stage 1256 passed
  when rerun twice in isolation; the gate classified it as unclassified
  because stale `quench-mkdtemp-*` artifacts and two generated stage files
  were present. Those explicit generated artifacts were removed, and the
  result is recorded as an artifact-cleanliness issue rather than a runtime
  failure.
- After the cleanup-tooling fix, the complete focused gate passed cleanly:
  1,968/1,968 stages, zero failures, zero retries, and 351 seconds in serial
  mode. This is the current authoritative focused-stage baseline.
- Fresh differential baseline completed at 2026-08-08T01:16:13Z across all
  4,682 Node parallel fixtures: 903 exact matches and 3,779 differences;
  501 both-failed, 174 Node-only failures, 531 output mismatches, 2,484
  quench-only failures, 89 timeouts, and 186 Node-environment-limited cases.
  The largest actionable owned cluster is HTTP (86 fixtures), followed by
  net (55), streams (42), and fs (38). The report is current and passes the
  platform-coverage audit.
- The next HTTP slice exposed the missing `ServerResponse#writeProcessing()`
  convenience API. Added it as a standards-shaped 102 wrapper over
  `writeInformation()` and added focused stage 2058.
- Added the HTTP socket `_handle.close()` surface and propagated server-side
  socket destruction to client `aborted`/`ECONNRESET` events; focused stage
  2059 covers the spurious-aborted lifecycle.
- Added the minimal readable `response.pipe()` bridge needed by real Node
  stream consumers; stage 2059 now exercises piping through the abort path.
- Limited socket-destroy abort propagation to incomplete client responses, so
  a normal completed response cannot be reported as spurious `aborted`.
- The next fs cluster showed that the VFS promise facade ignored access modes.
  It now delegates `fs.promises.access(path, mode)` to the same synchronous
  validation used by callback and sync APIs; focused stage 2060 covers the
  behavior.
- Stream `captureRejections` remains unresolved: a minimal paired
  `EventEmitter`/`Readable` probe shows the rejection callback is not delivered
  in the current foreign-runtime promise/event boundary. No focused passing
  stage was added for this behavior.

Recent verified milestones:

- Stage 2063 validates unref'ed timer behavior after HTTP server shutdown.
- Stage 2065 validates removal of pooled agent sockets when a server closes.
- Stage 2067 validates timer/HTTP close ordering; the upstream default-agent
  close fixture now passes.
- Stage 2069 is a passing real npm application probe using the installed
  `debug` package.
- Stage 2070 restores `MODULE_NOT_FOUND` error codes for unresolved package
  specifiers, matching Node's invalid-package require behavior.
- Grouped application/loader verification for stages 2069–2070 passes 2/2
  with zero retries and zero failures.
- The package loader now handles conditional `exports` maps and `.cjs`/`.mjs`
  entries. ESLint remains an unpassing larger application probe due to a
  separate recursive runtime incompatibility.

The fs copy cluster now also rejects asynchronous `cpSync()` filters with
`ERR_INVALID_RETURN_VALUE`; focused stage 2072 and the corresponding upstream
fixture pass.

Grouped focused verification for stages 2071–2072 passes 2/2 with zero
failures and zero retries.

Copy filters are now evaluated for the root source before destination
validation, allowing filtered-out copies to skip invalid destinations as Node
does. Focused stage 2073 and the upstream async skip-validation fixture pass.

After the referenced-handle/timer ordering fix, upstream
`test-http-agent-timeout.js` and `test-http-information-processing.js` both
complete successfully, resolving two entries from the earlier HTTP queue.

Fresh differential baseline completed at 2026-08-08T01:55:00Z across all 4,682
parallel fixtures: 914 exact matches and 3,768 differences; 500 both-failed,
175 Node-only failures, 530 output mismatches, 2,476 quench-only failures, 87
timeouts, and 191 Node-environment-limited cases. This improves the previous
baseline from 903 exact matches and 2,484 quench-only failures. The report is
fresh for the current fixture run; its focused-evidence freshness marker must
be refreshed by the next complete focused gate.

The focused marker is now current: the complete gate passes 1,976/1,976
stages with zero failures and zero retries in 321 seconds. The current
actionable queue remains HTTP (87 fixtures), net (55), streams (42), and fs
(38); platform coverage passes.

The next HTTP queue fixture, `test-http-chunked-smuggling.js`, depends on raw
`net.connect()` transport and incremental HTTP parser behavior. Quench's
current in-memory HTTP path does not expose that raw socket boundary, so it is
classified as a transport/parser gap rather than receiving a superficial HTTP
handler patch.

The fs cp cluster exposed missing `errorOnExist` handling for an existing
directory destination. Sync and async copy paths now raise
`ERR_FS_CP_EEXIST`; focused stage 2071 covers the promise API.

The fs cp cluster is now verified through stages 2071–2073: grouped focused
verification passes 3/3 with zero failures and zero retries. The upstream
fixtures for existing-directory `errorOnExist`, async-filter rejection, and
filtered invalid-destination validation all pass.

Focused stage 2074 adds symlink copy coverage for `dereference: true` and
`dereference: false`, async copy completion, and existing-directory
`errorOnExist`. The grouped fs-copy run for stages 2071–2074 passes 4/4 with
zero failures and zero retries. The corresponding upstream symlink,
destination-symlink, directory-exists, async-filter, and force/dereference
fixtures all pass.

The next fs-copy error cluster adds directory-to-file and self-subdirectory
validation, plus explicit `EEXIST` preservation when a symlink would overwrite
an existing file. Focused stage 2075 and the upstream directory-to-file,
symlink-over-file, and self-subdirectory fixtures pass. The upstream Unix
socket-copy fixture remains an explicit transport/liveness gap because its
server-created socket is not observable at the copy boundary in the current
runtime path.

Stages 2071–2075 pass as a grouped focused run: 5/5, zero failures and zero
retries.

The next net cluster corrected `net.isIP()` validation for malformed IPv6
compression, dotted tails, and scoped-address zones while retaining valid
IPv4, IPv6, and zone forms. Focused stage 2076 and upstream
`test-net-isip.js` pass. Stages 2071–2076 pass as a grouped focused run: 6/6,
zero failures and zero retries.

The IPv6 follow-up now validates the complete upstream `test-net-isipv6.js`
corpus. Stages 2077 and 2078 cover every invalid and valid address in that
fixture; both pass, as does the upstream fixture itself. The grouped focused
run for stages 2076–2078 passes 3/3 with zero failures and zero retries.

The net socket surface now exposes Node-compatible `bytesRead` and
`bytesWritten` counters and accounts for local string and Buffer writes.
Focused stage 2079 verifies the counter shape and accounting. The grouped
focused run for stages 2076–2079 passes 4/4 with zero failures and zero
retries. Full upstream byte-counter fixtures still require the runtime's
missing duplex socket/data-delivery model and remain explicitly unresolved.

Real-application coverage now includes the installed `chalk` package: focused
stage 2080 loads its CommonJS entry point and exercises chained styling. The
probe passes under quench-node and the equivalent host-Node smoke check passes;
the assertion avoids terminal-color output so it remains deterministic in CI.

Real-application stage 2081 now verifies the installed `ms` package through
its CommonJS entry point, covering string-to-duration and duration-to-string
conversions. The same deterministic assertions pass under quench-node and
host Node.

An ESLint Linter application probe remains unresolved: package loading reaches
the public API, but the first lint operation overflows the QuickJS stack inside
ESLint's parser/configuration path. A focused RegExp-flags surface probe (stage
2083) passes, so this is not being misclassified as a missing primitive.

Fresh full differential rebaseline completed at 2026-08-08T02:42:45Z against
all 4,682 parallel fixtures with zero worker failures: 924 exact matches and
3,758 differences (501 both-failed, 174 Node-only failures, 531 output
mismatches, 2,465 quench-only failures, 87 timeouts, and 191
Node-environment-limited cases). The current actionable owned queue is HTTP
(87), net (55), streams (42), and fs (35). The differential is complete and
authoritative; its focused-evidence freshness marker remains stale until the
next focused gate.

The next HTTP slice exposed missing `pause()` methods on incoming requests and
responses. Both now preserve the Node chainable surface; focused stage 2084
passes, as do upstream `test-http-pause-no-dump.js` and
`test-http-pause-resume-one-end.js`.

The next HTTP pipeline fixture exposed a missing `net.Socket.prototype.pipe()`
surface. It now forwards data, conditionally ends the destination, and returns
the destination as Node does. Focused stage 2085 and upstream
`test-http-many-ended-pipelines.js` pass.
