# Current compatibility push — Node 24 application compatibility and auditable baselines

This progress log follows the Node 24 application-runtime contract: upstream
Node tests are the primary oracle, focused stages are regression guards, and
Hono plus a representative CLI are release-facing application gates.
The authoritative Node, LLRT, Deno, WPT, and Test262 references are maintained
in `docs/authoritative-test-sources.md`.

## Verified progress

- Focused contracts: **2,071/2,071 passing** after the post-audit fixes.
- Live inventory: 58 canonical modules, 57 statically registered, one
  platform-limited runtime omission (`node:sea`), and 186 observed Node globals.
- Latest complete differential remains the previously recorded 4,682-fixture
  report; the refresh after the latest fixes reached 3,368/4,682 before its
  600-second wrapper timeout and is not counted as a complete baseline.
- `stream/iter` now covers broadcast cancellation/abort propagation,
  `fromWritable()`, and preservation of typed-array chunks in `array()` and
  `arraySync()`.

## Current verification

- The latest full focused audit discovered 2,071 stage directories; after
  resolving stages 456, 466, 475, 1866, 2135, and 2150, all discovered stages
  pass. The post-fix targeted audit and application probes are recorded below.
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
ESLint's parser/configuration path. A focused RegExp-flags surface probe (stage 2083) passes, so this is not being misclassified as a missing primitive.

Fresh full differential rebaseline completed at 2026-08-08T02:42:45Z against
all 4,682 parallel fixtures with zero worker failures: 924 exact matches and
3,758 differences (501 both-failed, 174 Node-only failures, 531 output
mismatches, 2,465 quench-only failures, 87 timeouts, and 191
Node-environment-limited cases). The current actionable owned queue is HTTP
(87), net (55), streams (42), and fs (35). The differential is complete and
authoritative; its focused-evidence freshness marker remains stale until the
next focused gate.

The complete focused gate then ran all 1,992 compatibility stages serially at
2026-08-08T02:49:07Z and finished at 02:54:53Z: 1,992 passed, zero failed,
zero retries, and zero policy-covered failures. This refreshes the focused
contract evidence for the current stage inventory; the broad differential
report still needs a post-gate rerun to carry the latest commit fingerprint.

The next HTTP slice exposed missing `pause()` methods on incoming requests and
responses. Both now preserve the Node chainable surface; focused stage 2084
passes, as do upstream `test-http-pause-no-dump.js` and
`test-http-pause-resume-one-end.js`.

The next HTTP pipeline fixture exposed a missing `net.Socket.prototype.pipe()`
surface. It now forwards data, conditionally ends the destination, and returns
the destination as Node does. Focused stage 2085 and upstream
`test-http-many-ended-pipelines.js` pass.

A reduced keep-alive agent reproducer is covered by focused stage 2086 and
passes, including the upstream sequence of calling `req.end()` after
`http.get()`, reuse after `IncomingMessage.destroy()`, and one created socket.
The exact upstream `test-http-client-abort-keep-alive-destroy-res.js` still
misses one harness callback, so the broader agent lifecycle remains unresolved
pending a closer callback-order comparison.

The stream drop/take public contract is covered by focused stage 2088:
sync-source drop/take, numeric-string and boolean coercion, zero-count behavior,
and negative-count validation all pass. The full upstream
`test-stream-drop-take.js` fixture still exposes an async/infinite-iterator
completion mismatch; no superficial method shim is being used to classify it
as fixed.

The portable fs-access contract is covered by focused stage 2087: valid read
access, invalid and out-of-range mode validation, missing-path `ENOENT`, and
callback/promise completion all pass. The full upstream `test-fs-access.js`
fixture remains platform-limited because it changes UID and branches on root
permissions; its callback mismatch is not treated as portable API evidence.

The fs copy validation slice now rejects the incompatible option pair
`dereference: true` plus `verbatimSymlinks: true` with
`ERR_INCOMPATIBLE_OPTION_PAIR`. Focused stage 2090 and upstream
`test-fs-cp-sync-incompatible-options-error.mjs` pass.

WebCrypto reflection coverage now includes focused stage 2092 for an HMAC
`CryptoKey`: public getters remain usable while `Object.getOwnPropertyNames`,
`Object.getOwnPropertySymbols`, and `Reflect.ownKeys` stay empty. The broader
upstream CryptoKey fixture still has an unresolved callback/lifecycle mismatch
across its ECDSA, RSA-PSS, and AES cases.

The same shared validator is confirmed across callback, sync, and promise
copy APIs: existing upstream option-validation fixtures pass, and focused
stage 2091 verifies the incompatible pair through both `fs.cp()` and
`fs.promises.cp()`.

Focused stage 2093 isolates ECDSA P-256 generation, key metadata, signing,
and verification; the complete contract passes under quench-node. This
narrows the remaining upstream CryptoKey discrepancy to its combined
multi-algorithm/lifecycle path rather than basic ECDSA support.

Focused verification for stages 2090–2095 completed at 2026-08-08T03:02:03Z:
6/6 stages passed serially with zero failures, zero retries, and zero
policy-covered failures. This covers the fs.cp option-pair checks and the
HMAC, ECDSA, RSA-PSS, and AES-GCM WebCrypto contracts added in this slice.

Focused stage 2095 independently verifies AES-GCM 128-bit key generation,
encryption, and decryption. It passes under quench-node, narrowing the
remaining upstream CryptoKey failure further to combined fixture sequencing
or reflection/lifecycle bookkeeping.

Focused stage 2094 independently verifies RSA-PSS 2048-bit generation,
signing, and verification with SHA-256 and a 32-byte salt. It passes under
quench-node, further narrowing the upstream CryptoKey mismatch to combined
fixture sequencing or the remaining AES/lifecycle path.

Focused stages 2096–2097 complete the four-algorithm matrix (HMAC, ECDSA,
RSA-PSS, and AES-GCM): generation for all keys and getter/reflection checks
for every key pass under quench-node. The original upstream fixture still
reports a missing completion callback, so the remaining discrepancy is now
isolated to its harness/lifecycle integration rather than algorithm support or
CryptoKey own-property shape.

Focused verification for stages 2090–2097 completed at 2026-08-08T03:04:23Z:
8/8 stages passed serially with zero failures, zero retries, and zero
policy-covered failures. This includes the complete four-algorithm WebCrypto
matrix and the fs.cp callback/promise option-pair contracts.

Focused stage 2098 adds canonical WebCrypto usage handling to the fallback
CryptoKey path: duplicate usages are removed and the returned list follows
Node's order for generated, imported, and cloned keys. The focused stage
passes. The upstream `test-webcrypto-deduplicate-usages.js` still ends with
`Callback 0: expected 1 calls, got 0`, so its combined lifecycle mismatch is
recorded separately from the now-covered usage-list contract.

Focused stage 2099 covers the HTTP server request socket's parser-owned
`data` listener, which Node exposes through `req.socket.listenerCount("data")`.
The runtime now installs that listener and forwards request-body chunks to the
server-side socket before request dispatch completes. The focused contract
passes, and `test-http-dump-req-when-res-ends.js` advances beyond its original
listener assertion. Its remaining callback is the raw `net.createConnection()`
data path, which still requires a real duplex net-to-HTTP transport model.

Focused stage 2100 adds `http.Agent` validation for `maxTotalSockets`: invalid
types raise `ERR_INVALID_ARG_TYPE`, non-positive and `NaN` values raise
`ERR_OUT_OF_RANGE`, and `Infinity` remains accepted. The focused contract and
upstream `test-http-agent-maxtotalsockets.js` both pass.

Focused stage 2101 covers the stream destruction invariant that `Readable`
`resume()` and `pause()` are no-ops after `destroy()`. The focused contract
passes. The broader upstream `test-stream-destroy.js` advances but still has
unresolved error-delivery assertions in its combined destroy matrix; those are
not being represented as fixed by this narrower contract.

Focused stage 2102 adds observable HTTP Agent request-slot bookkeeping:
requests beyond `maxSockets` now appear in `agent.requests[name]` while an
active slot is occupied and are removed as responses finish. The focused
queue contract passes, as do the existing `test-http-agent.js`,
`test-http-agent-maxtotalsockets.js`, and `test-http-abort-client.js` fixtures.
`test-http-agent-destroyed-socket.js` advances to a later socket lifecycle
assertion, so the complete destroyed-socket contract remains open.

The Agent lifecycle slice now gates queued request dispatch on the active slot,
adds the public request-socket `destroy()` surface, and emits the corresponding
non-keep-alive free notifications. Focused stage 2103 and the existing Agent
fixtures pass. `test-http-agent-destroyed-socket.js` advances to its final
socket-close countdown after server shutdown; transport close propagation is
still unresolved.

Real-application verification reran the installed `ajv`, `debug`, `chalk`, and
`ms` probes; all four pass under quench-node. A new host-Node Prettier smoke
probe now imports through nested conditional exports, resolves its absolute
ESM metadata, loads the Babel plugin, and formats source successfully under
quench-node. Focused stage 2104 and the equivalent host-Node formatting probe
both pass.

Focused stage 2105 adds Node-compatible `module.createRequire()` filename
validation for file URLs, absolute paths, relative paths, HTTPS URLs, and
invalid objects. The focused contract and upstream
`test-module-create-require.js` both pass.

Focused stage 2106 fixes percent-encoded Unicode file URLs in
`module.createRequire()`. Both the focused multibyte path contract and
upstream `test-module-create-require-multibyte.js` pass, along with the base
createRequire fixture.

Focused stage 2107 covers module metadata needed by real CommonJS packages:
local modules now maintain `module.children` and child `parent` links, and
`process.config.variables.node_module_version` is exposed as a positive
integer. Upstream `test-module-children.js` and `test-module-version.js` both
pass.

Focused stage 2108 implements POSIX `module._nodeModulePaths()`, including
ancestor lookup directories and the root/node_modules boundary behavior used
by CommonJS package resolution. The focused contract and upstream
`test-module-nodemodulepaths.js` both pass.

Stage 2109 keeps `.node` files on the native-addon path: the CommonJS loader
now reports `ERR_DLOPEN_FAILED` instead of evaluating a binary addon as
JavaScript. The focused native-addon error contract and upstream
`test-module-loading-error.js` pass.

Focused stage 2110 implements `Module._stat()`: it returns `1` for
directories, `0` for files, and a negative value for missing paths. The
focused contract and upstream `test-module-stat.js` both pass.

Focused stage 2111 exposes the top-level CommonJS `module` object plus
`require.cache` and `require.extensions`. The main module has a null parent and
the expected filename, matching Node’s entrypoint surface. The focused
contract and upstream `test-module-parent-deprecation.js` both pass.

Focused stage 2112 dispatches registered `require.extensions` handlers while
loading local CommonJS files and shares the table with `module._extensions`.
Registered compound suffixes are considered during extensionless resolution,
and `require.cache` invalidation reloads local modules. The focused contract
and upstream `test-module-multi-extensions.js` both pass.

Focused stage 2113 adds Node-compatible validation for
`Module.setSourceMapsSupport()`, including the boolean options `nodeModules`
and `generatedCode`. The focused contract and upstream
`test-module-setsourcemapssupport.js` both pass.

Focused stage 2115 implements `module._initPaths()` and `module.globalPaths`,
including `NODE_PATH` delimiter parsing and shared `Module.globalPaths` state.
The focused contract and upstream `test-module-globalpaths-nodepath.js` both
pass.

Focused stage 2114 restores legacy bare-package resolution: a package
directory without `package.json` now falls back to `index.js` and related
extensions. The focused contract and upstream
`test-module-circular-symlinks.js` both pass through the package lookup path.

Focused stage 2117 adds `require.resolve()` and `require.resolve.paths()` for
relative, absolute, and bare package lookups, reusing the same resolver as
`require()` without evaluating the target. The focused contract passes.
The upstream `test-require-resolve-opts-paths-relative.js` fixture also passes.

The resolver now recognizes built-in modules in `require.resolve()` and
returns `null` from `require.resolve.paths()` for those names, matching Node’s
non-filesystem resolution contract. The broader `test-require-resolve.js`
fixture remains open around custom `options.paths` ordering.

Focused stage 2118 removes descendant duplicates from explicit
`require.resolve(..., { paths })` lookup lists, matching Node’s handling of
nested `node_modules` entries. The focused path-order contract passes.
The broader `test-require-resolve.js` fixture advanced past its path-order
assertions and identified a trailing-slash package entry resolution gap
(`no_index/`), fixed in stage 2119 below.

Stage 2119 fixes package-main resolution for trailing-slash package names,
preserves filesystem paths while using realpaths only for cache identity, and
matches Node’s `require.resolve()` argument errors and relative lookup paths.
The focused contract and upstream `test-require-resolve.js` both pass.

The post-2119 module sweep found and corrected a cache invalidation regression
for custom `require.extensions`: deleting `require.cache[file]` now reloads
the module exactly once. Upstream `test-module-multi-extensions.js` passes;
symlinked circular exports remain a separate open lifecycle issue.

Focused stage 2120 makes `execFileSync(process.execPath, [missing-entry])`
fail with `MODULE_NOT_FOUND`, matching Node’s CLI behavior. The focused
contract and upstream `test-module-main-fail.js` both pass.
The same error contract now also passes `test-module-main-preserve-symlinks-fail.js`,
including CLI flags before the missing entrypoint.

Focused stage 2121 fixes `Readable.destroy(error)` custom `_destroy` dispatch:
the original error reaches the hook, returned errors emit once, and close/callback
ordering remains asynchronous. The focused contract targets the remaining
upstream `test-stream-readable-destroy.js` lifecycle gap.
The upstream fixture now advances through custom-destroy assertions and stops
at its legacy `Readable.call(this)` subclass pattern, which requires converting
the compatibility constructor from an ES class to a callable constructor.

Focused stage 2122 adds a callable public `Readable` façade while preserving
the internal class implementation. Legacy `Readable.call(this)` subclasses
now initialize successfully; the focused callable-constructor contract passes.
The upstream readable-destroy fixture now advances past that legacy
constructor case and stops at a later `push()`-after-destroy error semantic.

Focused stage 2123 aligns `Readable.push()` after destruction with Node: it is
a no-op returning `false`, rather than throwing. The focused contract targets
the next assertion in the upstream readable-destroy fixture.

Focused stage 2124 makes `Readable.unshift()` after destruction a no-op
returning `false`, matching Node and preventing data delivery to destroyed
streams.

Focused stage 2125 makes `Readable.push(null)` after destruction a no-op,
preventing delayed custom-destroy callbacks from emitting a spurious `end`.
The upstream fixture now advances to a later custom-destroy callback error
identity assertion.

Focused stage 2126 isolates custom `_destroy` callbacks that return an error;
the error identity is preserved in `readable.errored` and the emitted error.
The focused contract passes, while the full upstream fixture still has a
separate combined lifecycle mismatch.

Focused stage 2127 isolates the basic `Writable.destroy()` close and error
contracts; both pass. The full upstream writable-destroy failure is therefore
in a later combined/custom lifecycle case and remains separately classified.

Focused stage 2128 isolates custom writable `_destroy` error identity and
passes. Focused stage 2129 defers custom writable destroy completion to a
microtask, matching Node’s asynchronous close/error/callback boundary. The
focused timing contract passes; the full upstream fixture remains open.

Focused stage 2116 uses `module.globalPaths` when resolving bare packages, so
packages provided through `NODE_PATH` load without a local `node_modules`
entry. The focused global-package contract passes.

Focused stage 2131 adds a callable public `Writable` façade for legacy
`Writable.call(this)` subclasses, matching the corresponding `Readable`
compatibility behavior. The focused constructor contract passes. The upstream
writable-destroy fixture advances through its legacy constructor case; later
combined lifecycle failures remain separately classified.

Focused stage 2134 fixes auto-destroy ownership for the callable `Readable` and
`Writable` façades. Their hidden implementation instances no longer retain
error listeners that can reset the public stream's shared state. The focused
readable error-state contract passes, and upstream
`test-stream-readable-destroy.js` advances past the lost `errored` identity to
the next `push()`-after-EOF behavior.

Focused stage 2135 catches exceptions thrown from an internally invoked
`Readable._read()` and emits them as stream errors, matching Node's behavior
when a read implementation calls `push()` after EOF. The focused contract
passes, and upstream readable-destroy testing advances to AbortSignal error
name compatibility.

Focused stage 2136 implements `stream.addAbortSignal()` and abort-aware
`Readable` construction, destroying the public stream with an `AbortError`
and `ABORT_ERR`. The focused abort contract passes. The upstream fixture now
reaches a remaining combined abort/read lifecycle mismatch.

Focused stage 2139 prevents deferred `Readable` data delivery after
destruction, including listeners added after the stream is destroyed. The
focused no-data-flush contract passes; this closes one component of the
remaining abort/read lifecycle mismatch.

Stages 2144–2147 add focused coverage for automatic stream destruction across
readable, writable, transform, and pipe-error paths. All four contracts pass,
and the upstream `test-stream-auto-destroy.js` fixture passes as well.

Focused stage 2149 makes the public `Duplex` constructor callable without
`new`, and adds the expected `objectMode` state fields for readable and
writable sides. The focused contract passes. Upstream `test-stream-duplex.js`
advances past the callable-constructor assertions and now reaches the separate
`Duplex.fromWeb()` bridge gap.

Focused stage 2150 implements `Duplex.fromWeb()` using Web Stream readers and
writers, including asynchronous read, write, close, and cancellation/error
propagation. The focused bridge contract passes, and upstream Duplex testing
advances to the separate `Duplex.toWeb()` bridge gap.

Focused stage 2151 implements the basic `Duplex.toWeb()` conversion, exposing
Web readable and writable streams backed by the Node duplex. The focused
contract passes, and upstream Duplex testing advances through the basic bridge
to a later multi-case/BYOB callback-count mismatch.

Focused stage 2152 verifies `Duplex.toWeb(..., { readableType: "bytes" })`
with a BYOB reader and writable-side callback. The contract passes, isolating
the remaining upstream mismatch to repeated conversion/deprecation sequencing
rather than byte reads themselves.

Focused stage 2153 verifies repeated `Duplex.toWeb()` conversion after the
duplex has ended, including immediate readable closure. Focused stage 2154
fixes Duplex readable/writable side isolation so writable chunks are not
emitted as readable `data`. Both contracts pass; upstream Duplex testing now
advances beyond its initial exit-state assertion to a later missing callback.

Focused stage 2155 verifies that a Duplex writable write still reaches its
callback after the readable side has ended, including byte/BYOB conversion.
The contract passes. Duplex auto-destroy now waits for the writable side when
the readable side ends; the upstream fixture retains a later sequencing
callback mismatch.

Focused stage 2157 adds side-specific Duplex option handling for
`readableObjectMode`, `writableObjectMode`, `readableHighWaterMark`, and
`writableHighWaterMark`. The focused contract and upstream
`test-stream-duplex-props.js` both pass.

Focused stage 2158 aligns `Duplex({ readable: false }).push()` with Node's
asynchronous `ERR_STREAM_PUSH_AFTER_EOF` error event. Focused stage 2159
suppresses `end` emission for disabled readable sides. Both contracts pass,
and upstream `test-stream-duplex-readable-writable.js` passes.

Focused stage 2160 adds Blob support to `Duplex.from()` through the Blob Web
Stream, and stage 2161 invokes callable sources and handles async-function
rejections plus `ERR_INVALID_RETURN_VALUE`. Both focused contracts pass;
upstream `test-stream-duplex-from.js` advances to an object-wrapped readable
adapter case.

Focused stage 2162 verifies Node readable objects nested in `Duplex.from()`;
stage 2163 routes nested and direct Web Streams through the Web adapter and
preserves disabled sides. Both focused contracts pass, and upstream
`test-stream-duplex-from.js` advances through the Web input cases to a later
side-state assertion.

Focused stage 2164 verifies readable-only and writable-only direct Web Stream
inputs preserve the correct Duplex side flags. The focused contract passes;
the remaining upstream failure is now in later mixed object/error propagation
cases.

Focused stage 2166 updates the public `readable` state to `false` when EOF is
emitted, matching Node's post-`end` Duplex state. The focused contract passes;
upstream `test-stream-duplex-from.js` advances to a later callback-count
failure.

Focused stage 2167 covers `Duplex.from()` with a shared object-mode
PassThrough, including pause/resume backpressure and end/close delivery. The
focused pipeline contract passes; the full upstream fixture's remaining
callback-count discrepancy is not reproduced in isolation.

Focused stage 2168 propagates a readable-side error to both the derived Duplex
and its writable source, preserving error identity. The focused contract
passes; the upstream fixture still reports the separate PassThrough callback
11 mismatch.

Focused stage 2169 adds the own `Duplex.prototype.writableFinished` property
and verifies its transition through `finish`. The focused contract and
upstream `test-stream-duplex-writable-finished.js` both pass.

Fresh differential baseline completed on 2026-08-08 against the current
runtime binary across all 4,682 parallel fixtures: 998 exact matches, 2,447
Quench-only failures, 526 output mismatches, 462 both-failed cases, 172
Node-only failures, and 77 timeouts. The largest Quench-only prefixes are
`http2` (265), `http` (239), `tls` (160), `fs` (154), and `stream` (140).
The first `fs-access` candidate is environment-sensitive under the local
root/user setup and is not being treated as a portable compatibility fix.

Focused stage 2171 aligns stream `eventNames()` ordering for Node’s priority
events and hides the stream’s internal auto-destroy error listener when no
user error listener is present. The focused contract and upstream
`test-stream-event-names.js` both pass.

Focused stage 2172 aligns stream error-once behavior: `Readable.push()` after
EOF emits one asynchronous error when observed, while still throwing without
an error listener, and repeated invalid `Writable.write()` calls emit only one
error. The focused contract and upstream `test-stream-error-once.js` both
pass.

- Stage 2173: pass `captureRejections` options into writable event emitters so rejected async `drain` listeners surface as stream errors and destroy the writable, matching Node's `test-stream-catch-rejections.js`.
- Stage 2174: align writable completion ordering so length is reduced and `drain` is emitted before the write callback, allowing nested writes to produce the same two-drain sequence as Node.
- Stage 2175: add the exported `stream.destroy()` helper's default `AbortError` behavior while preserving direct stream `.destroy()` semantics.
- Stage 2176: verify the initial writable destroy lifecycle cases: close-only destruction and destruction from inside `_write()` with error propagation. The complete upstream writable-destroy matrix still has later lifecycle failures.
- Stage 2177: implement `Writable.prototype._undestroy()` state restoration so an auto-destroyed writable can run `final` and finish again, matching the upstream regression.
- Stage 2178: verify custom writable `_destroy()` callbacks can swallow the original destroy error while still emitting `close`, matching the upstream writable-destroy matrix.
- Stage 2179: verify a custom writable `_destroy()` callback can replace the error and that `error`/`close` remain deferred with updated error state.
- Stage 2180: verify repeated writable `destroy()` calls preserve the first error and emit one deferred error before `close`.
- Stage 2181: verify an asynchronous custom writable `_destroy()` error remains deferred, preserves the pending state across a second destroy call, and emits only once.
- Application gate verification: the installed Chalk, `ms`, Ajv, debug, and Prettier npm probes all pass under the current runtime binary on 2026-08-08. This confirms the existing representative npm application gates remain green while stream lifecycle work continues.
- Focused regression verification on 2026-08-08: `tools/check-focused-stages.sh` passed all 8 stages in its configured 2090–2097 window. `tools/check-focused-policy.sh` reported the existing 8 historical conflict entries and no new failure list; this command does not cover every stage number under `tests/node-compat`.
- Stage 2182: distinguish internal auto-destroy error listeners from user listeners, restoring Node's synchronous `ERR_STREAM_PUSH_AFTER_EOF`/`ERR_STREAM_DESTROYED` throws and recording the asynchronous EOF error state. Stages 456, 475, 2135, and 2172 pass after this fix.
- Full focused-stage audit on 2026-08-08 covered 2,071 stage directories: 2,065 passed and 6 failed (456, 466, 475, 1866, 2135, 2150). After Stage 2182, 456, 475, and 2135 pass; 466 remains a no-custom-destroy event-ordering issue, 1866 a Duplex data issue, and 2150 a liveness timeout after assertions pass.
- Stage 2183: preserve `new.target` through the `Duplex` compatibility wrapper so subclass `_read()`/`_write()` methods remain on the constructed prototype. Upstream-derived stage 1866 now passes.
- Stage 2184: flatten the no-custom-destroy completion path so writable `close` and destroy callbacks remain deferred but run before a zero-delay timer, matching stage 466 and preserving stages 2175–2181.
- Stage 2185: replace the Web Streams reader's infinite microtask polling with resolver-based pending reads. Stage 2150 now exits cleanly after delivering its data, and related Web/Duplex stages 2151–2153, 2160, 2163, and 2164 remain green.
- Post-fix targeted audit on 2026-08-08: all six previously failing focused stages (456, 466, 475, 1866, 2135, 2150) pass together. The five representative npm application probes and the Rust test suite also pass.
- Stage 2186 isolates short `AbortSignal.any()`/`AbortSignal.timeout()` delivery through `events.once()`; it is the next probe for the longer upstream timeout fixture.
- Stage 2187 isolates the same timeout behavior through `Promise.race()` and `assert.rejects()`; both short probes pass, while the long upstream case still exposes timer ordering.
- Stage 2189 updates stale focused stage 475 to current Node behavior: `Readable.push()` after destroy returns `false` rather than throwing. This resolves the sole failure in the complete 2,073-stage audit; Node CLI behavior was checked directly.
- Complete focused audit on 2026-08-08: `tools/check-focused-stages.sh` passed **2,073/2,073** stages with zero retries and zero failures in 232 seconds. `tools/check-focused-policy.sh` reported zero unclassified failures; the eight policy entries are historical conflict records only.
- Broad local gate verification on 2026-08-08: `tools/check-all-tests.sh` passed the Rust workspace tests (2/2) and the complete 2,073-stage focused suite with zero retries and zero failures. Generated `cp-error-*` artifacts were removed from the worktree after the run.
- Complete parallel differential baseline on 2026-08-08: all 4,682 parallel fixtures produced results with no worker failures. Results were 1,004 exact matches, 461 both-failed, 2,448 Quench-only failures, 526 output mismatches, 172 Node-only failures, 71 timeouts, and 192 Node-environment-limited fixtures. The report is `target/compat/differential-parallel.json` and was generated at commit `a3eb3bac`.
- Stage 2190 aligns HTTP 304 responses: preserve an explicitly supplied `Content-Length` header while still suppressing the response body.
- Stage 2191 adds tolerant replacement-character handling for invalid UTF-8 in `Buffer.toString()`, fixing the decoder exception exposed by `test-http-buffer-sanity.js`.
- Stage 2192 emits `readable` before in-memory HTTP response data, allowing `stream.finished()`/destroy lifecycle consumers to observe and cancel responses like Node.
- Stage 2193 preserves query strings for string, legacy `url.parse()`, and `URL` targets passed to `http.get()`.
- Stage 2194 aligns client-request `.destroy()` with Node's `socket hang up`/`ECONNRESET` error before `close`.
- Stage 2195 preserves Node's distinction between client-request `.destroy()` and `.abort()`: direct destroy reports `ECONNRESET` without setting `aborted`.
- Stage 2196 wires `http.request()`'s external `AbortSignal` to a deferred `AbortError`/`ABORT_ERR` destroy while preserving Node's `aborted` state.
- Stage 2197 makes `ClientRequest.abort()` mark the request destroyed without changing its no-error abort event behavior.
- Stage 2198 suppresses the later synthetic connection-reset error after an explicit `ClientRequest.abort()`, matching Node's no-error abort path.
- Stage 2199 suppresses the implicit reset error when a client request is destroyed after its response has already been delivered.
- Stage 2200 adds `OutgoingMessage` destroy state (`destroyed`, `closed`, `errored`) and deferred `close` delivery.
- Stage 2201 adds server-response destroy state and suppresses the synthetic abort error for `ServerResponse.destroy()`.
- Stage 2202 preserves the server-response `errored` identity while deferring `close` without emitting a duplicate error.
- Stage 2203 preserves client response delivery when a server calls `end()` before `destroy()`.
- Stage 2204 consumes a custom Agent connection's returned Duplex and delivers its chunked HTTP response to the client.
- Upstream `test-http-client-readable.js` now passes after accepting a complete first chunk before a custom socket is ended, matching the fixture's `readable = false` ordering.
- Stage 2205 validates Node's rejection of an array-valued `host` request header with `ERR_INVALID_ARG_TYPE`.
- Stage 2206 rejects request paths containing unescaped characters outside Node's Latin-1 path range with `ERR_UNESCAPED_CHARACTERS`.
- Upstream `test-http-client-invalid-path.js` now passes after validating `options.path` before URL normalization.
- Stage 2208 applies the same unescaped-path validation before `http.get()` normalizes object options through a URL.
- Stage 2209 validates non-string `http.request()` `hostname` and `host` options with Node-compatible `ERR_INVALID_ARG_TYPE` messages.
- Stage 2210 preserves custom URL properties such as `url.headers` when `http.request()` receives a WHATWG URL object.
- Stage 2210 also verifies normal client-request completion emits `close` after the response ends.
- Stage 2211 avoids inherited `hostname` access when `http.request()` receives null-prototype options, matching Node's own-property handling.
- The null-prototype upstream fixture now advances past inherited-option access to a later response callback-count discrepancy.
- Stage 2212 reproduces two sequential requests, including a null-prototype options object, to isolate the remaining response callback gap.
- Stage 2213 emits the client request's `socket` event with its in-memory socket so timeout/listener consumers can observe it.
- Upstream `test-http-client-set-timeout-after-end.js` now passes with the socket event and zero-timeout listener behavior.
- Stage 2214 tracks socket timeout values across request creation, connect, and `setTimeout()` rescheduling.
- The upstream timeout fixture now reaches a duplicate-timeout callback-count discrepancy after socket timeout scheduling is aligned.
- Upstream `test-http-client-set-timeout.js` now passes after `ClientRequest.destroy()` cancels its socket timer.
- Stage 2221 exercises eight concurrent requests and `new http.Server(handler)`; all server-side `aborted` events arrive in the focused stress contract.
- Stage 2222 preserves the optional listener argument in static `events.listenerCount()`.
- Stage 2217 emits response-level `timeout` events when `IncomingMessage.setTimeout()` configures its socket timer.
- Stage 2218 supports the `server.listen(callback)` overload with an ephemeral port.
- Upstream response-timeout testing now advances past the listen overload to a later response-timeout callback discrepancy.
- Upstream `test-http-client-response-timeout.js` now passes after keeping response socket timeout timers active.
- Stage 2219 propagates client-request aborts to the server-side `IncomingMessage` as Node's `aborted` event.
- The upstream abort stress fixture still has a separate multi-request callback-count discrepancy after abort propagation.
- Stage 2220 guarantees client-request destruction emits `close` only once under repeated/error races.
- Upstream `test-http-client-abort-destroy.js` now passes after the close-once guard; combined timeout still reports duplicate `timeout` delivery.
- Representative npm application recheck on 2026-08-08: Ajv, debug, Chalk, `ms`, and Prettier probes all exit successfully under the current runtime binary.
- Complete focused audit on 2026-08-08 after the listener-count fix: **2,105/2,105** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2216 preserves `http.request()` results as `instanceof http.ClientRequest`, matching Node's public type contract.
- The upstream timeout fixture now advances past the request-instance assertion to a later duplicate destroy/timeout callback discrepancy.
- Upstream `test-http-hostname-typechecking.js` now passes with synchronous host-option validation.
- Stage 2207 supports `new http.ClientRequest(serverAddress, callback)` by routing it through the same in-memory request path as `http.request()`.
- Upstream `test-http-client-input-function.js` now passes with the `ClientRequest` constructor path.
- Broad local gate verification on 2026-08-08: `tools/check-all-tests.sh` passed Rust tests (2/2) and all 2,105 focused stages serially with zero retries or failures. The generated `cp-error-*` artifact was removed.
- Refreshed parallel differential baseline on 2026-08-08 at the current commit: all 4,682 fixtures completed with zero worker failures. Results: 1,017 exact matches, 461 both-failed, 2,431 Quench-only failures, 525 output mismatches, 172 Node-only failures, 76 timeouts, and 191 Node-environment-limited fixtures. The report is `target/compat/differential-parallel.json`.
- Stage 2223 cancels remaining internal `AbortSignal.timeout()` timers when `AbortSignal.any()` has already aborted, preventing a long secondary timeout from extending process lifetime. The focused 9-second/110-second timer regression passes and Rust tests remain 2/2 green. The upstream `test-abort-controller-any-timeout.js` now reaches its assertion but still fails in the foreign `node:test` promise-assertion path; it is not counted as fixed.
- Follow-up isolation of the upstream timeout discrepancy: an exact focused reproduction using `node:test` confirms `Promise.race()` resolves through its 10-second fallback instead of observing the 9-second abort event, while the equivalent top-level focused timer contract passes. The remaining defect is in async test-wrapper scheduling, not timeout reason matching; no upstream pass is claimed.
- Timer-order diagnosis: Quench currently queues each timer as an independent pending job. A timer callback that resolves a promise queues its reaction behind already-queued later timer jobs, so a 10ms abort promise can lose to a 30ms fallback even though `signal.aborted` is already true. Node drains promise jobs between timer callbacks; the next runtime fix should centralize timer scheduling around the earliest due timer and return to the job queue after each callback.
- Stage 2225 optimization: add an ASCII fast path to the shared `TextEncoder` implementation so large ASCII `Buffer.from()` conversions write directly into a pre-sized byte array instead of growing an intermediate array. The large `fs.promises.appendFile()` focused stage 1858 now completes within the normal 30-second stage timeout.
- Full focused audit after the encoder optimization on 2026-08-08: **2,106/2,106** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests remained 2/2 green, the filesystem append upstream fixture passed with an extended timeout, and representative Ajv, debug, and Prettier npm probes passed. The upstream 9-second AbortSignal/node:test fixture remains separately blocked by the documented timer-order issue.
- Stage 2225 adds Node-compatible timer handle primitives: timeout and interval handles expose stable numeric `Symbol.toPrimitive()` values, numeric and string IDs work with `clearTimeout()`, and `clearImmediate()` ignores timeout handles. The focused contract and upstream `test-timers-to-primitive.js` plus `test-timers-invalid-clear.js` pass. `test-timers-unref.js` still exposes the separate unref scheduling gap.
- Focused audit after timer-handle changes on 2026-08-08: **2,108/2,108** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2226 adds `Symbol.dispose` to timeout, interval, and immediate handles and marks disposed handles as `_destroyed`. The focused contract and upstream `test-timers-dispose.js` pass, including the requirement that `clearImmediate()` and disposal distinguish handle types.
- Focused audit after timer-disposal changes on 2026-08-08: **2,108/2,108** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests remained 2/2 green.
- Stage 2227 adds `timers/promises.scheduler.wait()` and `.yield()` to the promise-timer surface, backed by the existing promise timer operations. The focused ordering contract passes. The upstream scheduler fixture reaches a later validation assertion and is not claimed fully fixed.
- Focused audit after the timers/promises scheduler addition on 2026-08-08: **2,109/2,109** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2228 aligns `timers/promises.scheduler`: illegal construction throws `ERR_ILLEGAL_CONSTRUCTOR`, detached methods reject invalid receivers with `ERR_INVALID_THIS`, and pre-aborted waits reject with `ABORT_ERR`. The focused contract and upstream `test-timers-promises-scheduler.js` now pass.
- Focused audit after scheduler validation alignment on 2026-08-08: **2,110/2,110** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests and representative Ajv, debug, and Prettier npm probes also passed.
- Stage 2229 attaches Node's `util.promisify.custom` metadata to callback timers so `promisify(timers.setTimeout)` and `promisify(timers.setImmediate)` are identical to their `timers/promises` counterparts. The focused identity contract passes; upstream timeout/immediate promisified fixtures advance past identity to a later callback-lifecycle discrepancy.
- Focused audit after timer promisify metadata on 2026-08-08: **2,111/2,111** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2230 replaces the fetch stub with a local HTTP bridge using the existing `http.request()` implementation. It supports URL/method/headers/body requests and returns status, headers, and text through the existing `Response` surface. The focused POST/response contract passes.
- Focused audit after the fetch bridge on 2026-08-08: **2,112/2,112** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests remained 2/2 green.
- Stage 2231 expands the fetch `Response` surface with `bodyUsed`, `arrayBuffer()`, `bytes()`, and `blob()` while preserving JSON/text/clone behavior. The focused response-body contract passes.
- Focused audit after fetch response-body expansion on 2026-08-08: **2,113/2,113** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests remained 2/2 green.
- Stage 2232 expands `Request` with signal propagation, `bodyUsed`, body consumption, and clone-after-consume rejection. The focused request-body contract passes.
- Focused audit after fetch request-body alignment on 2026-08-08: **2,114/2,114** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests remained 2/2 green.
- Stage 2233 aligns Blob constructor validation for non-array sources and exposes the Web Streams `internal/webstreams/util` `kState` symbol with controller state. The focused Blob validation contract passes. The full upstream Blob matrix still reaches a native Blob type-normalization discrepancy (`type: {}`), which remains documented rather than claimed fixed.
- Focused audit after Blob/Web Streams state changes on 2026-08-08: **2,115/2,115** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2234 normalizes native Blob `type` values to Node's lowercased string behavior for direct global and `buffer.Blob` construction. The focused contract passes and Rust tests remain 2/2 green. The upstream Blob matrix still observes the pre-cached native `buffer.Blob` path for `type: false`, so `test-blob.js` remains unresolved and is not claimed fixed.
- Focused audit after native Blob type normalization on 2026-08-08: **2,116/2,116** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2235 routes CommonJS local-module `require()` calls through the final module-surface normalization hook, covering modules loaded indirectly by a fixture. The focused local-module Blob contract passes. The upstream Blob fixture still captures a native constructor at an earlier entry-module boundary (`type: false` remains `""`), so it is not claimed fixed.
- Focused audit after local-module finalization on 2026-08-08: **2,118/2,118** stages passed serially with zero retries or failures; policy verification reported zero unclassified failures and only the eight historical conflict records. Rust tests remained 2/2 green.
- Stage 2237 aligns `stream/web` exports with the global Web Streams constructors and supplies the missing global constructor identities used by Node's Web Streams surface. The focused identity contract and upstream `test-global-webstreams.js` both pass. Rust tests remain 2/2 green.
- Focused audit after Web Streams constructor identity alignment on 2026-08-08: **2,119/2,119** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2238 adds `ReadableStream` controller error propagation, rejecting pending and future reads with the original error identity. The focused error contract passes. The upstream Web Streams pipeline fixture now advances beyond its missing `controller.error()` call to a separate stream-adapter mismatch, so it is not claimed fully fixed.
- Focused audit after Web Streams controller error propagation on 2026-08-08: **2,120/2,120** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2239 converts Web Readable/Writable/Transform objects to the existing Node Duplex bridge before invoking `stream.pipeline()`. The focused Web pipeline adapter contract passes. The upstream pipeline fixture still reaches a later multi-stage adapter/lifecycle discrepancy, so it is not claimed fully fixed.
- Focused audit after the Web Streams pipeline adapter addition on 2026-08-08: **2,121/2,121** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures and only the eight historical conflict records.
- Stage 2241 implements TransformStream-backed `TextEncoderStream` and `TextDecoderStream`, including incremental UTF-8 handling, validation, state getters, and reader wake-up/close behavior. Upstream `test-whatwg-webstreams-encoding.js` passes.
- Stage 2242 implements zlib-backed `CompressionStream` for gzip, deflate, deflate-raw, and brotli, extends `DecompressionStream` validation/raw support, and adds Node-compatible tags/getters. Upstream `test-whatwg-webstreams-compression.js` passes; the focused compression contract passes.
- Focused audit after text and compression Web Streams support on 2026-08-08: **2,123/2,123** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2243 exposes Node's `internal/webstreams/adapters` through the existing public Web-to-Node bridge, adds reader/writer locking, cancellation, `pipeTo`/`tee` invalid-state behavior, source cancellation propagation, and `kState` state/error transitions. The focused adapter contract passes. Upstream adapter testing now advances through lock, destroy, error, and state assertions to a later callback lifecycle discrepancy; it is not claimed fully fixed.
- Focused audit after Web Streams adapter state support on 2026-08-08: **2,124/2,124** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Follow-up adapter verification makes the internal adapter factories constructible where Node permits `new`, validates invalid readable/writable pairs, propagates writable abort/close state through the Duplex bridge, and passes upstream `test-whatwg-webstreams-adapters-to-streamduplex.js` and `test-whatwg-webstreams-adapters-to-streamwritable.js`. The stream-readable adapter fixture remains partial at a later callback lifecycle assertion. The focused suite remains **2,124/2,124** and Rust tests 2/2.
- Stage 2244 adds demand-driven `ReadableStream` `pull()` scheduling, allowing adapters and readers to request source data when their queue is empty. The focused pull contract passes. The upstream readable-adapter fixture still reports a later callback-count/lifecycle discrepancy and is not claimed fully fixed.
- Focused audit after ReadableStream pull scheduling on 2026-08-08: **2,125/2,125** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2245 implements `ReadableStream.tee()` with shared source reads and propagates asynchronous `start()` failures into stream errors. It also begins Web Stream support in `stream.finished()` for readable/writable state completion. The focused tee contract passes; upstream finished testing still exposes a writer callback-order discrepancy and remains unresolved.
- Focused audit after Web Streams tee/start-error support on 2026-08-08: **2,126/2,126** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2246 adds state-aware Web Stream support to `stream/promises.finished()`, including writable completion waiters and readable error rejection. The focused promise-finished contract passes. The callback-based upstream `test-webstreams-finished.js` still has an interaction-specific writer ordering discrepancy, which remains documented rather than claimed fixed.
- Focused audit after promise-based Web Stream completion support on 2026-08-08: **2,127/2,127** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2247 adds `ReadableStream` high-water-mark and custom-size queue accounting, including dynamic `controller.desiredSize` as chunks are enqueued and consumed. The focused queue contract and upstream `test-webstreams-queue-wraparound.js` pass.
- Focused audit after Web Streams queue accounting on 2026-08-08: **2,128/2,128** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2249 implements `ByteLengthQueuingStrategy` and `CountQueuingStrategy` with Node-compatible high-water-mark, size, and receiver validation. The focused strategy contract passes. The upstream Web Streams coverage fixture remains blocked at the engine-level `internal/webstreams/util.isPromisePending` primitive, which cannot be faithfully inferred synchronously from portable JavaScript.
- Focused audit after Web Streams queuing strategies on 2026-08-08: **2,129/2,129** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2250 wires `stream.addAbortSignal()` into Web Stream controllers, makes `finished()` wait for readable queue drain/error, and exposes reader `closed` promises with AbortError rejection. The focused abort contract passes. The upstream abort fixture advances through its first abort/read/closed assertions but still has a later multi-stream callback discrepancy.
- Focused audit after Web Streams abort propagation on 2026-08-08: **2,130/2,130** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Stage 2251 aligns the runtime application entry contract by exposing `process.argv[0]` as `quench-node` while preserving Node's `process.argv0` value. The focused argv contract, repository smoke application, full focused audit, and Rust tests all pass.
- Focused audit after the process argv application fix on 2026-08-08: **2,131/2,131** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Real-application probe recheck on 2026-08-08: installed Ajv schema validation, Chalk formatting, debug initialization, `ms` duration parsing, Prettier Babel formatting, and the repository smoke application all pass under the current runtime binary. ESLint remains an unresolved application gate; `new Linter().verify()` reaches a `RegExp`/`String.replace` recursion (`Maximum call stack size exceeded` in the parser path), requiring a dedicated differential trace.
- Stage 2253 resumes readable demand after a paused destination drains by invoking the existing readable-start path instead of an unused pump hook. The focused pause/drain contract passes. The upstream backpressure stress fixture still reports a deeper high-water-mark/read cadence mismatch; it is not claimed fully fixed.
- Focused audit after readable backpressure resumption on 2026-08-08: **2,132/2,132** stages passed serially with zero retries and zero failures; policy verification reported zero unclassified failures. Rust tests remained 2/2 green.
- Backpressure trace on 2026-08-08: the large-chunk stress contract drains correctly without the Node `common.mustCall` wrapper, but with the wrapper around `_read()` it receives only the first of 11 expected calls. This narrows the remaining mismatch to callback-wrapper interaction with readable demand scheduling; no speculative change was retained.
- Follow-up state probe on 2026-08-08: immediately after the wrapped `_read()` returns, Quench has `reading=true`, `paused=true`, 40 buffered chunks, and `writableNeedDrain=true`; the next read is suppressed by the `reading` guard before the drain/resume cycle. The diagnostic stage was removed and no speculative guard change was retained.
- Stream-consumers isolation on 2026-08-08: upstream `test-stream-consumers.js` still reports `Callback 5: expected 1 calls, got 0`, while a focused five-way concurrent PassThrough consumer contract passes. This indicates an interaction later in the full fixture rather than a basic `stream/consumers.text()` completion defect; no speculative implementation change was retained.
- Stage 2255 makes `Readable` slice cleanup (`take`/`drop` and transform iterators) invoke the source iterator's `return()` without awaiting it, so a bounded read does not wait on an async generator's unresolved next-item promise. The focused unresolved-generator contract passes. Upstream `test-stream-drop-take.js` still reports the same later callback discrepancy under its concurrent/infinite-stream matrix, so full upstream compatibility is not claimed.
- Stage 2256 keeps `stream/iter` broadcast `writeSync()` and `writevSync()` synchronous: strict budget exhaustion now returns `false` instead of leaking the pending Promise used by asynchronous `write()`. The focused budget contract passes. Upstream `test-stream-iter-broadcast-basic.js` still has a separate Promise.all lifecycle discrepancy, so the full fixture is not claimed fixed.
- Stage 2257 defers `dgram.bindSync()`'s `listening` event until the next turn so listeners attached after the synchronous bind observe it; Stage 2258 delivers datagrams to bound destination sockets even when the sender is still unbound. Focused contracts pass, and upstream `test-dgram-bind-sync.js` passes.
- Stage 2259 aligns asynchronous dgram bind retries: `bind()` callback-only overloads are normalized and `EADDRINUSE` is emitted asynchronously instead of thrown synchronously. The focused retry contract and upstream `test-dgram-bind-error-repeat.js` and `test-dgram-bytes-length.js` pass.
- Stage 2260 adds `common.localhostIPv4`, `common.localhostIPv6`, and `common.hasIPv6` to the Node test-support shim. Upstream `test-dgram-address.js` now passes with loopback address assertions. A broader dgram sweep still reports unresolved handle, connected-send, lookup, and argument-validation gaps, which remain queued separately.
- Stage 2261 aligns synchronous dgram connections for IPv6: the default peer is `::1`, implicit binding registers the socket for delivery, and `remoteAddress().family` follows the socket type. The focused IPv6 contract and upstream `test-dgram-connect-sync.js` pass.
- Stage 2262 normalizes string elements in dgram array payloads before buffer concatenation. The focused connected-send contract and upstream `test-dgram-connect-send-multi-string-array.js` pass.
- Stage 2263 applies dgram `send()` offset/length slicing before delivery, including zero-length datagrams. The focused empty-packet contract and upstream `test-dgram-connect-send-empty-packet.js` pass.
- Stage 2264 recognizes the unconnected `send(buffer, offset, length, port)` overload and normalizes ArrayBuffer views before slicing, fixing numeric-port misclassification and DataView crashes. The focused offset/port contract passes; upstream `test-dgram-send-default-host.js` advances to a later aggregate message-count discrepancy and remains unresolved.
- Stage 2265 includes `size` in dgram receive metadata (`rinfo`), allowing Node-style byte accounting for delivered packets. The focused metadata contract and upstream `test-dgram-send-default-host.js` now pass.
- Stage 2266 aligns dgram send validation: unconnected port ranges, connected overload rejection, buffer offset/length bounds, scalar buffer error details, and invalid buffer-list diagnostics. The focused port contract and upstream `test-dgram-send-bad-arguments.js` pass.
- Stage 2267 aligns the Node test-support `common.mustSucceed(fn, exact)` call-count overload and bigint address diagnostic formatting. The focused helper contract and upstream `test-dgram-send-address-types.js` pass.
- Stage 2269 accepts Node's numeric `dns.lookup(host, family, callback)` overload. The focused DNS contract passes. The dgram default-lookup fixture still exposes a separate module-object/stub propagation issue for mismatched-family lookup, so no dgram lookup pass is claimed.
- Stage 2270 propagates dgram `sendBlockList` failures through asynchronous `connect()` and `send()` callbacks as `ERR_IP_BLOCKED`, suppresses packets rejected by `receiveBlockList`, and models implicit sender loopback addresses. The focused blocklist contract and upstream `test-dgram-blocklist.js` pass.
- Stage 2271 routes dgram binding through the exposed internal handle lookup hook and prevents a deferred bind from completing after `close()`. The focused lifecycle contract and upstream `test-dgram-close-during-bind.js` pass.
- Stage 2272 validates dgram `signal` options and closes sockets on live or pre-aborted `AbortSignal`s. The focused signal contract and upstream `test-dgram-close-signal.js` pass.
- Default-address isolation on 2026-08-08: a focused two-socket UDP4/UDP6 contract with the same `common.mustCall` callback shape passes and reports `0.0.0.0`/`::`; upstream `test-dgram-bind-default-address.js` still fails with an opaque QuickJS exception. No speculative address change was retained.
- HTTP-agent isolation on 2026-08-08: upstream `test-http-agent-maxsockets-respected.js` reports `Callback 1` (the server-listen wrapper) was never called, while a standalone `http.createServer().listen()` probe completes and returns a valid address. The remaining issue is interaction-specific to the HTTP-agent fixture; no unrelated server-listen change was retained.
- Stage 2274 adds dgram hostname-resolution failure callbacks (`ENOTFOUND`), `removeListener()`, and correct removal of `once()` wrappers. The focused host-error contract and upstream `test-dgram-send-cb-quelches-error.js` pass, including callback-vs-error-event behavior.
- Stage 2275 validates dgram bind addresses and asynchronously reports `EADDRNOTAVAIL` with Node-compatible `code`, `address`, and message fields for non-local IPv4/IPv6 addresses. The focused contract and upstream `test-dgram-error-message-address.js` pass.
- Stage 2276 treats the callback-only `dgram.send(message, port, callback)` overload as having no hostname, avoiding function-hostname errors. The focused callback-overload contract and upstream `test-dgram-send-address-types.js` pass.
- Stage 2277 narrows connected dgram destination rejection so `send(buffer, offset, length, callback)` remains valid, and tracks asynchronous bind lookup with `_bindPending` for `connectSync()` validation. The focused connected-offset contract and upstream `test-dgram-connect-sync.js` and `test-dgram-close-during-bind.js` pass.
- Stage 2278 resolves asynchronous dgram `localhost` binds to `127.0.0.1` through the handle lookup path. The focused localhost-bind contract and upstream `test-dgram-close.js` pass.
- Stage 2279 invokes the internal dgram send hook and maps nonzero results to callback/error-event metadata. The focused handle-send contract passes; upstream `test-dgram-send-error.js` still has a later interaction-specific unexpected-message discrepancy.
- Stage 2280 schedules default dgram lookup completion on the next turn, makes close idempotent, and prevents `listening` from firing before listeners attach. The focused lifecycle contract and upstream `test-dgram-listen-after-bind.js` and `test-dgram-close-during-bind.js` pass.
- Follow-up Stage 2281 exposes `UV_UNKNOWN` and maps its system error name to `UNKNOWN`, completing internal dgram send-hook error metadata. Upstream `test-dgram-send-error.js` now passes; the focused handle-send contract remains passing.
- Stage 2282 suppresses successful dgram send callbacks when the destination socket is already closed, while retaining callbacks for delivered packets and errors. The focused closed-destination contract and upstream `test-dgram-oob-buffer.js` and `test-dgram-send-address-types.js` pass.
- Stage 2283 adds the internal dgram `_createSocketHandle()` and `udp_wrap.UDP` surfaces for unbound and local bound handles. The focused handle contract and upstream `test-dgram-create-socket-handle.js` pass; fd-adoption variants remain separately unsupported.
- Stage 2284 adds lightweight UDP/TCP fd identity and UDP fd adoption/rejection behavior for internal dgram handles. The focused fd contract and upstream `test-dgram-create-socket-handle-fd.js` pass.
- Stage 2285 rejects repeated dgram `bind()` calls synchronously with `ERR_SOCKET_ALREADY_BOUND`, including while an earlier bind lookup is pending. The focused contract and upstream `test-dgram-bind.js` pass.
- Stage 2286 supports dgram `bind({ fd })` validation, reporting `EEXIST` for occupied UDP descriptors and `ERR_INVALID_FD_TYPE` for TCP descriptors. The focused contract and upstream `test-dgram-bind-fd-error.js` pass.
- Stage 2287 extends raw UDP handle identity, address metadata, `bind6()`, `getsockname()`, and fd adoption. Focused stages 2284 and 2286 remain passing; upstream `test-dgram-bind-fd.js` advances but still stalls at a later callback lifecycle assertion, so full fd-bind compatibility is not claimed.
- Stage 2288 preserves raw UDP handle address/port metadata by fd and reuses it during adoption. The focused handle contracts remain passing; upstream `test-dgram-bind-fd.js` still reports a later callback-count mismatch and remains unresolved.
- Stage 2289 registers adopted dgram fds as live bound sockets and preserves callback-only `bind({ fd }, callback)` overloads. Upstream fd-bind testing now reaches packet exchange, with a remaining concurrent raw-fd identity mismatch (`60001` vs `60000`).
- Stage 2290 assigns unique synthetic ephemeral ports to raw UDP handles, eliminating concurrent fd-adoption cross-delivery. Upstream `test-dgram-bind-fd.js` now passes.
- Stage 2291 rejects UDP payloads larger than 65,507 bytes with Node-compatible `EMSGSIZE` callback/error metadata. The focused size contract and upstream `test-dgram-msgsize.js` pass.
- Stage 2292 allows same-port dgram binds when `reusePort: true`. The focused reuse-port contract and upstream `test-dgram-reuseport.js` pass.
- Stage 2293 validates and invokes dgram custom lookup callbacks for hostname binds, with Node-style null/string/boolean type diagnostics. The focused custom-lookup contract passes; upstream `test-dgram-custom-lookup.js` now advances to a separate callback-wrapper `TypeError` in its custom lookup path.
