# Compatibility throughput and differential triage

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Goal

Prioritize changes that collapse multiple failures into one declaration or
generic adapter. Differential clusters should map to reusable data and code,
not a growing collection of fixture-specific wrappers.

Increase Node 24 application-compatibility implementation throughput by 2–5x without weakening
the readable-polyfill, local-verification, or no-GitHub-CI requirements.

The current one-mismatch-at-a-time workflow is useful for minimizing
regressions, but it serializes discovery, diagnosis, implementation, and
verification. The next tooling investment is a differential test pipeline
that compares Node and quench-node, clusters failures, and produces an
owned work queue.

## Workstreams

### 1. Differential corpus runner

Add a local tool that executes each selected upstream fixture with both Node
and quench-node and records:

- fixture path;
- exit status;
- normalized stdout and stderr;
- timeout or crash state;
- first failure signature;
- API cluster and owner, when known.

The runner must support deterministic timeouts, bounded child-process cleanup,
repeatable ordering, and persisted results. It must not embed JavaScript source
inside JavaScript string literals.

### 2. Failure clustering and triage

Normalize failures into stable categories:

- missing module or API;
- assertion/value mismatch;
- exception type, code, or message mismatch;
- asynchronous ordering or timing;
- serialization/encoding mismatch;
- environment or platform limitation;
- flaky or nondeterministic result.

Emit a ranked queue of unique signatures rather than requiring a developer to
rediscover the first failing fixture manually. Store minimized reproductions
as readable focused stages.

### 3. Cluster-sized slices

Keep focused files below the enforced size and complexity limits, but group
related cases into one API slice where practical. A slice may contain several
readable stage files covering one cluster such as URL parsing, streams/events,
filesystem/path, crypto/network, or module loading.

Do not use one-file-per-mismatch as a hard rule when the failures share one
general implementation. Retain one focused regression per distinct contract.

### 4. Generated scaffolding

Add a stage template/generator that creates source files, imports the shared
Node test helpers, adds exit-event assertions, and updates the ledger without
generating opaque JavaScript strings. Generated files remain ordinary,
formatted, reviewable JavaScript.

### 5. Parallel ownership

Partition work into up to five non-overlapping API streams:

1. URL/WHATWG and encoding;
2. streams/events and async ordering;
3. filesystem/path/process/module loading;
4. crypto/network/OS integration;
5. harness, globals, and test infrastructure.

Each stream must use an isolated worktree or branch, own distinct files, and
merge only verified commits. Shared polyfill edits require explicit ownership
handoff to avoid conflicting changes.

### 6. Coverage and retrospectives

Extend local reports to show:

- upstream fixtures executed;
- pass, fail, unsupported, platform-limited, known-conflict, and not-tested
  counts;
- pass rate by API prefix/cluster;
- unique failure signatures;
- regressions since the previous run;
- completed and unassigned work items.

Do not call this an API percentage. Node tests are not one-to-one with Node
APIs; API coverage remains `unmeasured`. The manifest targets all applicable
Node 24 trees, and release readiness also requires zero unexplained failures
in all six workload-class application gates.
The authoritative source roles and implementation order are maintained in
`docs/authoritative-test-sources.md`.

## Execution order

1. Build the differential runner on a small representative corpus.
2. Add normalized result persistence and failure clustering.
3. Add prefix/cluster reports and regression comparison.
4. Add stage scaffolding generation.
5. Partition the backlog across isolated workstreams.
6. Batch related failures into general polyfill slices.
7. Re-run the full corpus locally and record a retrospective after each batch.

## Success criteria

- A corpus run produces a deterministic Node-vs-quench result file.
- Repeated failures collapse into stable signatures with representative
  fixtures.
- A developer can select a cluster and receive a bounded, ordered work queue.
- Five independent workstreams can operate without overlapping ownership.
- Full local corpus runs clean up timed-out child processes.
- Focused stages remain Prettier-, ESLint-, and `git diff --check`-clean.
- No GitHub Actions or other GitHub CI is introduced.

## Expected impact

Differential triage should remove the serial “find the next mismatch” cost.
Cluster-sized slices should reduce stage bookkeeping, and isolated ownership
should provide the largest throughput gain when multiple contributors work in
parallel. Together these changes target roughly 2–5x throughput; this is a
working hypothesis to validate with before/after corpus and cycle-time data.

## Status

### 2026-08-09 differential refresh

The full parallel differential corpus completed 4,682/4,682 fixtures with
zero worker failures in 694 seconds. The fresh report records 1,268 matches,
3,414 non-matches, 391 both-failed cases, 241 Node-only failures, 570 output
mismatches, 2,141 Quench failures, 71 timeouts, and 192 environment-limited
fixtures. The top owned queue signatures are HTTP (145), net (47), stream
(45), fs (36), and REPL (35). The report is current at
`target/compat/differential-parallel.json`; compatibility work remains open.

The throughput work is now tracked and extended by task 020. The ranked audit
identified fragmented evidence and missing release-gate orchestration as the
largest immediate process costs; those improvements are implemented in
`tools/compat-goal-audit.sh` and `tools/check-application-stages.sh`.

The HTTP queue recently verified stage 2045 against the upstream
`test-http-client-upload.js` fixture. Request chunk boundaries and response
encoding are now covered by a focused regression and match the Node oracle in
the in-memory transport.

Stage 2046 also verifies Node-style console `%s` and `%j` formatting. The
upstream HEAD-request fixture now has matching formatted content, but still
exposes separate output-spacing and duplicate-response-end differences in the
foreign-runtime harness; these remain queued rather than classified as fixed.

The fresh focused baseline passes 1,959/1,959 registered stages with zero
failures after the package-loader and ESM `fs/promises` fixes. This supersedes
the earlier 1,953/1,956 snapshot produced while those changes were still being
compiled.

The fresh full differential completed all 4,682 parallel fixtures with zero
worker failures: 898 matches, 2,492 quench-only failures, 529 output
mismatches, 502 both-failed, 174 Node-only, 87 timeouts, and 190
environment-limited fixtures. The report is
`target/compat/differential-current.json`.

The first HTTP queue slice after that report is now covered by stage 2048 and
upstream `test-http-abort-client.js`; server-response destruction matches the
client abort/error/close contract. Agent timeout and uninitialized-handle
behavior remain in the HTTP queue.

Stage 2049 verifies the missing keep-alive socket `free` event. This fixes the
agent's first reuse contract while leaving the broader timeout/reuse ordering
matrix for a subsequent slice.

Stage 2050 then verifies the public Agent path for manually seeded sockets with
partial handles; the corresponding upstream uninitialized-handle fixture now
passes.

Stage 2051 verifies that the agent pool is populated before the socket `free`
event reaches user listeners, allowing the next request to reuse the same
socket. The broader timeout fixture still has a custom timer branch queued.

Stages 2052–2053 verify custom timeout configuration and replacement after
socket destruction. These isolate the public socket subcontracts; the full
upstream four-block timeout/reuse lifecycle still requires further ordering
work.

In progress. `tools/diff-node-quench.sh` now executes a deterministic fixture
selection under both Node and quench-node, persists normalized stdout/stderr,
exit statuses, stable diagnostic signatures, fixture prefixes, and coarse
failure categories in a JSON report, and cleans up timed-out children. It
accepts either a fixture directory or one fixture file. `tools/compat-queue.sh`
groups the report by signature and prints the highest-volume fixtures with
representatives; when given a prior report, it also reports regressions and
resolved fixtures. `tools/compat-ownership.json` provides explicit stream
ownership and the small set of currently known platform-limited prefixes;
`compat-queue.sh` reports those separately from unclassified failures. The
next slice should add fixture-level capability probes and inventory reports.

The Node side of the differential runner uses
`tools/run-node-fixture.cjs`, which compiles each fixture as CommonJS from a
real module wrapper. This avoids the false `common`-harness failures caused by
running fixtures through `node -e`, where `require`, `module`, `exports`,
`__filename`, and `__dirname` become global properties. A remaining mismatch
after this isolation is therefore treated as fixture or runtime behavior,
rather than as an invocation artifact.

When both runtimes exit successfully for a fixture that imports `node:test`,
and only Node emits its reporter stream, the differential result records a
`node-test-reporter-suppressed` comparison and counts as a match. The result
retains this comparison mode so reporter differences remain auditable without
being misclassified as API failures.

Example:

```sh
tools/diff-node-quench.sh tests/node/test/parallel target/compat/differential.json
tools/compat-queue.sh target/compat/differential.json 25 target/compat/previous.json
```

The focused Buffer cluster now passes stages 217, 230, 263, 271, 274, and
1046 individually. Stages 1031 and 1041 expose a fixture-version conflict
with stage 271: they require legacy `new Buffer(number)` allocation, while
stage 271 requires current Node behavior that rejects the same one-argument
numeric constructor. One runtime behavior cannot satisfy both; fixture policy
must resolve this before claiming a 100% focused gate. Caller-specific
branching is excluded.

The open-flag fixtures also disagree on the platform constant for `as`:
stage 101 expects `1051713`, while stage 1320 expects `1053761`. This is
recorded as another versioned fixture conflict; one `stringToFlags` mapping
cannot satisfy both expectations.

Focused-stage measurement also needs artifact hygiene: filesystem fixtures
that create files in the repository root can make later stages fail with
`EEXIST`, permission, or descriptor errors when a prior interrupted run leaves
them behind. Such artifacts must be removed explicitly before classifying a
failure as a runtime gap; the focused runner should eventually isolate or
clean these paths automatically.

The same fixture-policy issue appears in `util.format`: stage 329 expects
`%O` object string values without quotes, while stage 1051 expects quotes for
the same `%O` contract. Both cannot be true simultaneously in one runtime;
this is tracked as a versioned fixture conflict rather than an implementation
gap.

An earlier focused baseline recorded 1,723/1,723. Historical conflicts are retained in
the policy file for provenance: stages 1031 and 1041 conflicted with stage 271 over numeric legacy
`Buffer` construction; stage 1033 conflicts with stage 1038 over exact
negative-offset error wording; stage 1051 conflicts with stage 329 over `%O`
formatting; stage 1320 conflicts with stage 101 over the platform `as` flag;
stages 1342 and 1343 conflict with stage 395 over SHA-1 exposure; stage 1345
conflicts with stage 520 over the TLS cipher list; and stage 1537 conflicts
with stage 1514 over empty-authority URL normalization. These are fixture
version/policy conflicts, not undocumented polyfill gaps; all are now resolved
against the installed Node CLI behavior.

The same policy boundary applies to upstream `test-url-parse-format.js`: its
whitespace-trimmed authenticated URL expectation conflicts with focused stage
282, which explicitly requires the legacy auth omission in `href`.

The focused gate is serial by default because the current
filesystem fixtures use repository-relative paths; concurrent execution can
create false `EEXIST` and symlink failures. The latest clean serial run is
reported 1,702/1,702 with zero failures at that point; the runner writes each
new result to
`target/compat/focused-latest.txt`.

The parallel differential gate performs a single binary preflight before
starting workers. It fails closed on any worker/report error and only merges
sorted, complete worker reports, recording `workers` and `timeout_seconds` in
the result metadata. A report with fewer fixtures than the selected manifest
is therefore an explicit failed run, not a misleading partial baseline.
The parallel differential runner now completes the full 4,682-file corpus.
Reports now also record start/finish timestamps, Node version, quench binary,
and source commit for reproducible provenance; a one-fixture metadata smoke
run verified these fields.
The refreshed report (`target/compat/differential-post-http.json`) contains
473 matches (10.10%), 44 output mismatches, 2,417 quench-only failures, 1,480
both-failed fixtures, 122 Node-only failures, and 146 timeouts across all
4,682 fixtures. The earlier authoritative report remains retained as a
baseline. The latest queue has 817 legacy signatures (1,552 classified queue
groups); 918 failures are explicitly platform-limited and 1,063 remain
unclassified. The unclassified
set is an explicit remaining API and harness triage queue, not silently
counted as compatibility. The queue classifier now groups by legacy
signature, fixture prefix, owner, status, and category, avoiding arbitrary
representative-fixture classification for mixed signatures. It reports
classified queue groups separately from legacy signatures, conflict counts,
and total-versus-displayed fixture counts. QUIC fixtures are explicitly
platform-limited because this host does not expose native QUIC transport and
event-loop integration.

The subsequent full run (`target/compat/differential-post-http-headers.json`)
contains 469 matches, 2,418 quench-only failures, 1,466 both-failed fixtures,
127 Node-only failures, 45 output mismatches, and 157 timeouts. Its four-match
regression versus the prior 473-match run is concentrated in filesystem and
crypto/permission fixtures and is treated as run-to-run environmental
variance until independently reproduced; the HTTP content-length and
header-array fixtures now pass directly.

DTLS is now explicitly classified as a platform-limited transport prefix;
against the current report this raises explicit platform coverage to 957
fixtures and leaves 1,042 unclassified for actionable triage. Ordinary TLS
validation fixtures remain unclassified/actionable rather than being hidden
behind the transport limitation.

An earlier full report (`target/compat/differential-current.json`) recorded
4,682 unique fixtures, 462 matches (9.87%), 2,204
quench-only failures, 1,376 both-failed, 123 Node-only failures, 42 output
mismatches, and 475 timeouts. It recorded run provenance metadata at that
point; reports must pass `tools/compat-report-status.sh` before being
treated as a current upstream baseline. Prior reports remain historical
comparisons.

The ESM host path now evaluates `.mjs` entries through rquickjs modules and
resolves relative `.mjs` imports, CommonJS `.js` default imports, and selected
`node:` builtin namespaces. Focused stages 1735--1739 and 1741 cover builtin,
relative, filesystem, `module.createRequire`, `import.meta.url`, and the Node
common ESM helper graph; full CommonJS named-export discovery and
dynamic/package imports remain open
compatibility work.

The `fs.cp` fallback now performs real file and recursive directory copies,
preserves symlinks unless `dereference` is requested, applies synchronous
filters, and supports callback, promise, and synchronous forms. Focused
stages 1744--1746 cover file, recursive/filter, and callback/filter contracts;
upstream fixture-specific helper/metadata differences remain under triage.

`fs.watchFile()` now returns a reusable watcher object with `ref`, `unref`,
`hasRef`, `listenerCount`, `close`, and listener removal through
`unwatchFile`; focused stage 1727 passes. The upstream ref/unref fixture now
reaches the missing `common.platformTimeout` harness helper, which is tracked
as harness infrastructure rather than misreported as a watcher API failure.

The first high-volume async-context slice now has a real `AsyncLocalStorage`
implementation covering `run`, `enterWith`, `exit`, `withScope`, `disable`,
`bind`, `snapshot`, `defaultValue`, and `name`; stage 1700 covers the
synchronous contract. Promise continuation context propagation remains an
explicit runtime gap, demonstrated by upstream
`test-async-local-storage-enter-with.js`, and is not counted as resolved.
The remaining await-context mismatch is at the rquickjs native microtask
boundary: native `await` continuations do not pass through the JavaScript
`Promise.prototype.then` hook, so complete fidelity requires engine-facing
microtask integration in the existing `quench-node` host.

AbortSignal composition now has focused coverage for `any`, `timeout`, abort
reasons, and `events.once` EventTarget delivery in stages 1706 and 1707. The
upstream `test-abortsignal-any.mjs` remains an async timer/harness mismatch;
the direct `events.once(signal, 'abort')` contract passes independently, so
this fixture is not silently classified as an API implementation success.

Assertion Error-cause comparison now covers nested Error, object, and explicit
`undefined` causes in focused stages 1701 and 1702, including Node's
multi-line object-cause diff rendering. The upstream
`test-assert-deep-with-error.js` fixture now passes.

The full upstream `test-assert-class.js` fixture now matches the local Node
CLI, including long-string and multiline `util.inspect(AssertionError)` and
`deepEqual` rendering. Async crypto signing and verification also match after
the callback implementation and `common/fixtures.readKey()` compatibility fix.
The upstream `test-assert-async.js` fixture now also passes after aligning
invalid thenable, invalid return, validation callback, and `doesNotReject`
error contracts.
Blob object URLs now have a shared in-memory registry with `createObjectURL`,
`resolveObjectURL`, and revocation semantics; the upstream
`test-blob-createobjecturl.js` fixture passes, and the fallback Blob provides
the `arrayBuffer`, `text`, and `slice` contracts used by that fixture.
AsyncHooks Promise lifecycle coverage now passes for Promise creation,
`promiseResolve`, trigger IDs, enable/disable, and the pre-existing-Promise
fast path. The focused runner also removes stale empty mkdtemp directories
recursively, eliminating the prior stage-1256 false failure.
Bounded differential slices now exactly match the Blob object-URL, Assert
async, Promise lifecycle, and AsyncResource constructor fixtures. The remaining
`test-async-hooks-async-await.js` mismatch is explicitly classified in
`tools/compat-ownership.json` as the native await/microtask boundary limitation.
The shared `buffer.atob`/`buffer.btoa` implementations now match the local Node
CLI for coercion, Latin-1 output, malformed Base64, and DOMException error
contracts; `test-btoa-atob.js` passes.

HTTP header validation now exposes Node-compatible public validators and the
`_http_common` token/invalid-character helpers, including undefined-header
value and non-Latin-1 rejection. Focused stage 1721 and the upstream
`test-http-header-validators.js` and `test-http-common.js` fixtures pass.

The HTTP `Agent` surface now models option state and Node's deterministic
`getName()` pool keys, including local address, family, and Unix socket cases;
focused stage 1722 and `test-http-agent-getname.js` pass. Real socket creation
remains an explicit host transport limitation rather than a fake successful
connection.

HTTP server lifecycle objects now expose `closeAllConnections`,
`closeIdleConnections`, `Symbol.asyncDispose`, and the internal
`kConnectionsCheckingInterval` state used by Node's async-dispose contract.
Focused stage 1723 and `test-http-server-async-dispose.js` pass; this does not
claim real socket transport support.

The in-process HTTP request object now exposes method, headers, header
mutation, body writes, `end`, and response events; focused stage 1724 passes.
The broader upstream header-array and content-length fixtures still expose
response-header fidelity gaps and remain in the actionable HTTP queue.

The full native-event `test-fs-watchfile.js` and BigInt variant are explicitly
classified as host-limited because the current polyfill does not synthesize
native filesystem watcher delivery; the ref/unref watcher object contract is
covered independently by upstream and focused stage 1727.

Child-process event/stream fidelity now exposes a public `ChildProcess`
constructor, validates spawn options, preserves stdout/stderr stream identity,
and emits the spawn, stream, exit, and close sequence without duplicate
wrapper events. Focused stage 1728 and upstream
`test-child-process-constructor.js` and `test-child-process-spawn-event.js`
pass.

Stage 1820 additionally restores Node's `net.createServer()` identity:
returned servers are `instanceof net.Server` while retaining the existing
event-emitter methods and non-networking behavior. The focused contract passes.

The adjacent upstream `test-fs-open-no-close.js` was also checked. Its
`fs.open()` callback executes, but the fixture fails because the synthetic
runtime does not deliver Node's process `beforeExit` lifecycle callback; this
is tracked as a process lifecycle gap rather than an `fs.open()` regression.

Stage 1818 adds host lifecycle emission of `process.beforeExit` before
`process.exit`, with focused ordering coverage. The upstream
`test-fs-open-no-close.js` now passes; the former failure was confirmed to be
the missing lifecycle event.

Stage 1819 normalizes the non-networking `net.createServer()` result to Node's
initial server shape: `listening: false`, null `address()`, undefined socket
limits, and chainable `ref()`/`unref()`/`close()` methods. The focused contract
passes; actual TCP listen/connect behavior remains explicitly unsupported.

The lifecycle boundary now suppresses `beforeExit` for explicit
`process.exit()` and rechecks pending jobs after a `beforeExit` emission. The
upstream `test-beforeexit-event-exit.js` passes; the more elaborate repeated
timer/listen fixture `test-process-beforeexit.js` still exposes a deeper timer
turn scheduling gap.

`spawnSync` now returns Node-shaped ENOENT errors (`code`, `errno`, `syscall`,
`path`, `spawnargs`) and output arrays, while retaining Buffer/string output
conversion. The upstream spawn-sync fixture now reaches the missing
`common.pwdCommand` harness fixture rather than failing at process-result
shape.

The shared `spawnSync` path now also handles `pwd` with `cwd`, Buffer versus
string encoding, and stable `output` arrays. Focused stage 1729 and upstream
`test-child-process-spawnsync.js` pass.

The shared `exec` path now expands `${ENV_VAR}` placeholders from
`common.escapePOSIXShell` and preserves UTF-8 stdout/stderr output for the
encoding contract. Focused stage 1730 and upstream
`test-child-process-exec-encoding.js` pass.

These classifications are machine-readable in
`tools/focused-compat-policy.json`; both focused gate scripts pass their
actual failed-stage list to `tools/check-focused-policy.sh`, which verifies
that every current failure is covered before a baseline is reported.

The latest clean focused run after the child-process and `IncomingMessage`
prototype fixes is 1,712/1,712 with zero failures and zero uncovered failures.

Focused stage 1749 covers two high-fanout assertion details: `ifError` treats
every value except `null` and `undefined` as an unwanted exception, and
`assert.throws` compares explicitly supplied falsy properties. Stages 1748
and 1749 both pass locally.

An earlier focused regression band through stage 1769 was 1/1 with zero
failures and zero uncovered failures. The refreshed inventory records 58
modules, 186 globals, 211 polyfill assignments, and 4,682 upstream parallel
fixtures. Platform coverage is explicit for 22 host-limited fixtures,
including raw HTTP/TCP protocol tests that require native socket transport.

The HTTP response state-machine work also brings upstream
`test-http-response-close.js`, `test-http-response-remove-header-after-sent.js`,
`test-http-response-statuscode.js`, `test-http-response-writehead-returns-this.js`,
`test-http-response-setheaders.js`, and `test-http-request-host-header.js` to
exact differential matches.

The latest focused additions include a Node-compatible `zlib.crc32` path
(stage 1728 and upstream `test-zlib-crc32.js`) and a complete bridge for the
Node test common ESM helper exports (stage 1747). The ESM bridge is now
validated at the import boundary; `test-fs-cp-async-filter-function.mjs` still
has a later generic harness failure and remains open rather than being counted
as a pass.

Queue ranking now prioritizes owned and unclassified actionable clusters over
platform-limited groups. The focused-stage discovery gate includes both `.js`
and `.mjs` contracts. Inventory and platform ownership audits currently pass;
the broad differential report is marked stale whenever source, focused stages,
or ownership changes, so it must be regenerated before publishing upstream
pass-rate claims.

Focused stage 1770 closes the zlib immutability contract: `zlib.codes` is a
frozen object and the exported property is non-writable, matching upstream
`test-zlib-const.js` exactly.

Focused stage 1771 adds Node-style `ERR_INVALID_ARG_TYPE` validation for
non-buffer zlib inputs, including exact received-value message formatting;
both the focused contract and upstream `test-zlib-not-string-or-buffer.js`
now pass.

Focused stage 1772 aligns zlib callback validation order: invalid input is
rejected synchronously before a missing callback is reported. This fixes the
first mismatch in `test-zlib-invalid-input.js`; its later spoofed typed-array
range checks remain open.

Stages 1773--1774 add spoofed typed-array range coverage across the synchronous
compression methods. The upstream fixture now reaches a later stream-private
method gap; that remaining failure is tracked separately from input range
validation.

Stage 1775 adds `_processChunk`, `_chunkSize`, `_outOffset`, and `close()` to
the compatibility stream surface. Its private-range contract passes; the
upstream `test-zlib-invalid-input.js` now passes after stage 1776 adds
decompressor error-event delivery on invalid stream data.

Stage 1777 introduces shared numeric constructor-option validation for zlib
streams (`chunkSize`, `windowBits`, `level`, and `memLevel`). The focused
contract now covers strategy, dictionary, `Z_MAX_CHUNK`, and `params()` as
well; upstream `test-zlib-deflate-constructors.js` passes.

Stage 1778 adds shared `flush` and `finishFlush` option validation to the
stream constructors; both the focused contract and upstream
`test-zlib-flush-flags.js` pass.

Stage 1780 adds callback-aware `write()`, `flush()`, and `resume()` methods to
the compatibility zlib streams. Its focused callback contract passes; the
upstream write-after-end fixture still has a later asynchronous harness
failure and remains open.

Stage 1781 adds the standard `Promise.withResolvers()` helper, with a focused
settlement contract passing. The write-after-end fixture still has an
independent test-runner/mock-context failure after this harness primitive.

Stages 1782--1783 add the `node:test` mock-function context and reproduce the
write-after-end callback contract exactly in a focused fixture. The upstream
fixture now passes after matching Node’s `mock.callCount()` method shape.

Stage 1784 adds callback-aware zlib stream `close()` behavior; its focused
contract and upstream `test-zlib-close-after-write.js` both pass.

Stage 1785 adds the Node-compatible `_closed` state transition on decompressor
errors and close callbacks. Focused stage 1785 and upstream
`test-zlib-close-after-error.js` pass.

Stage 1786 preserves constructor identity for `createDeflateRaw()` and
`createInflateRaw()`; its focused contract and upstream
`test-zlib-create-raw.js` pass.

Stage 1789 adds gzip member concatenation and
`ERR_TRAILING_JUNK_AFTER_STREAM_END` handling for `rejectGarbageAfterEnd`.
Its focused contract passes; `test-zlib-type-error.js` now reaches a later
Brotli/decompression-stream rejection gap.

Stage 1790 adds equivalent Brotli trailing-member detection for
`rejectGarbageAfterEnd`; its focused contract passes. The upstream type-error
fixture now reaches the missing `stream/web` `DecompressionStream` behavior.

Stage 1791 adds the basic `stream/web` pipeline surface (`pipeThrough`, async
iteration, and `DecompressionStream`) with a passing focused pipeline contract.
The upstream trailing-data fixture now reaches deeper stream/error semantics.

Stages 1794--1795 verify single- and multi-chunk deflate rejection through the
web-stream pipeline. Stage 1796 adds gzip trailing-byte detection for
`rejectGarbageAfterEnd`; all three focused contracts pass.

Stages 1797 and 1798 cover all upstream gzip and Brotli trailing-data vectors
through `DecompressionStream`; both focused format contracts pass. The
remaining upstream mismatch is confined to the combined test-runner path.

Stage 1235 now covers `createUnzip()` detection for both gzip and the host's
raw-DEFLATE representation. The stream buffers one-byte writes, joins
concatenated gzip members, and emits `finish` after `end`; its focused
contract and upstream `test-zlib-unzip-one-byte-chunks.js` pass.

Focused stage 1724 remains intentionally unresolved: it asserts a string from
an HTTP response `data` event without calling `setEncoding()`, while Node's
local CLI emits a `Buffer` by default. Stage 1753 independently verifies the
Buffer-default contract. The implementation keeps the Node behavior until the
conflicting focused contract is reconciled; no platform exemption hides it.

The Blob stream path is now normalized onto the local web-stream shim even
when the host exposes a native Blob implementation; focused stages 1791,
1793, and 1796 remain green after this boundary correction.

Stage 1792 adds the standard `Array.fromAsync` global helper, allowing the
upstream decompression-stream tests to execute further. Its focused async
collection contract passes; the upstream fixture still has a deeper rejection
propagation mismatch.

Stage 1793 reproduces the upstream deflate trailing-data vector through the
new web-stream pipeline; its focused rejection contract passes. The upstream
fixture still differs in multi-chunk/native Blob stream propagation.

The parallel differential runner now computes the repository fingerprint once
per run and shares that immutable JSON snapshot with its per-fixture workers.
Previously every worker invocation of `diff-node-quench.sh` rescanned the
source, focused-stage, ownership, and selected fixture trees, adding repeated
hashing overhead without changing the merged report. Direct single-fixture
runs retain the independent fingerprint path; the optimization is bounded to
`diff-node-quench-parallel.sh` and preserves schema-2 report metadata.

`compat-report-status.sh` now forwards a caller-supplied fixture selection to
the platform auditor. The auditor remains strict for the default full corpus,
while bounded reports ignore ownership entries that cannot occur in their
selection; this keeps small differential slices auditable without weakening
the full-corpus stale-classification check.

The differential runner also launches the Node and quench sides concurrently
for each fixture by default (`QUENCH_DIFF_PARALLEL_SIDES=1`), reducing
per-fixture wall time while preserving process isolation and exact result
classification. Set `QUENCH_DIFF_PARALLEL_SIDES=0` for resource-constrained
or diagnostic runs; schema-2 reports record the selected mode as
`parallel_sides`.

The full focused sweep now reports 1,786 passing contracts and zero failures.
Stage 1724 was corrected to assert Node's Buffer-default HTTP response data
behavior; stage 1235 covers incremental and concatenated `createUnzip()`
decoding; and stage 1776 covers decompressor error delivery.

Stage 1729 adds the shared `ClientRequest.setTimeout()` surface, request
timeout state, callback registration, timer cleanup on response/destroy, and
`options.timeout` wiring. Its focused contract passes. Upstream timeout
fixtures still expose deeper synthetic-transport lifecycle gaps (notably
connect/close ordering and server-side idle behavior), so they remain
actionable rather than classified as platform limitations.

The highest expected 2--5x throughput gains are persistent quench workers with
fresh JS contexts, a content-addressed per-fixture result cache, and caching
the immutable Node side keyed by fixture and Node-version digests. Per-fixture
timing and adaptive scheduling should precede those changes so speedups are
measurable and timeout regressions remain visible.

Differential result records now include `duration_ms`, per-side duration
fields, and `node_timed_out`/`quench_timed_out` flags. The fields are preserved
by both single-run and parallel-run reports, and serial versus parallel
classification remains equivalent on bounded fixtures. This establishes the
telemetry baseline needed before introducing result caching or persistent
workers.

The decision-quality review identified the next missing evidence needed for
credible 2--5x claims: complete cache identity (including binary, runner,
timeout, environment, and mode digests), true per-side phase timestamps,
retry/flake history, structured failure frames/symbols, platform evidence
confidence, and focused-run provenance/completeness metadata. Current reports
must not be treated as proving cache safety or process-startup versus fixture
execution cost until those fields are persisted.

The common-module bridge now normalizes `.js`, `.mjs`, and `.cjs` suffixes when
resolving upstream helper imports. This aligns ESM relative imports such as
`../common/fs.js` with the existing CJS helper mappings; stages 1737, 1743,
and 1747 continue to pass after the change. Several async `fs.cp` fixtures
still fail later in their helper/filesystem setup and remain unresolved.

Stage 1800 adds Node-compatible `ERR_FS_CP_NON_DIR_TO_DIR` handling when a
file copy targets an existing directory. The focused contract passes; the
corresponding upstream ESM fixture still fails earlier in the shared helper
setup, so this behavior is covered locally without overstating upstream
coverage.

Stage 1801 adds destination-symlink replacement to the shared
`copyFileSync` primitive. The host copy operation follows symlinks, whereas
Node replaces a destination symlink when copying a file; removing only the
destination link before the host copy aligns that behavior without changing
ordinary file or directory handling. The focused contract and upstream
`test-fs-cp-sync-dereference.js` both pass.

Stage 1802 adds tracked-mode permission enforcement to `copyFileSync`, so a
destination made read-only through the public `chmodSync` surface produces
Node's `EACCES` error and remains unchanged. The focused contract and
upstream `test-fs-copyfile-respect-permissions.js` both pass.

Stage 1803 verifies ESM named-import interop for the shared `common/fs` helper
and `node:fs` constants/stat functions. The probe passes; remaining upstream
ESM `fs.cp` failures are therefore tracked as deeper copy semantics or
fixture setup issues rather than undocumented missing exports.

Stage 1806 adds named-export interop for `node:fs/promises`. Its focused
contract passes, and the upstream promise-copy options, nested-folder, and
mode-flag fixtures now pass; file-URL copying remains unresolved.

Stage 1808 adds a basic synchronous `fs.globSync()` implementation and
focused coverage. The full upstream glob fixture still requires richer
brace, globstar, symlink, exclusion, and `withFileTypes` semantics, so that
surface remains an actionable follow-up rather than an overstated pass.

The fresh full differential report (`target/compat/differential-latest.json`)
covers all 4,682 upstream fixtures and records 605 matches (12.92%), 4,077
non-matches, 2,366 quench-only failures, 1,025 both-failed fixtures, 158
Node-only failures, 311 output mismatches, and 217 timeouts. The queue audit
now reports 963 platform-limited and 1,011 unclassified non-matching fixture
results. The former prefix-wide DTLS classification was removed because the
current report contained DTLS non-matches without a quench-side failure;
those fixtures remain visible and actionable until a host limitation is
actually evidenced.

The differential runner now emits heartbeat progress such as completed
fixtures, failed workers, and elapsed seconds. A 16-worker full run completed
the 4,682-fixture corpus without worker failures in 342 seconds, making the
measurement path itself auditable while preserving the serial focused gate.

Differential report fingerprints now include binary, comparator, and Node
runner SHA-256 identities plus timeout and parallel-side mode. This closes a
cache-safety provenance gap identified by the throughput review; bounded
serial and parallel smoke reports both persist and validate these fields.

The post-change full differential rerun is current and complete: 4,682
fixtures, 605 matches, 4,077 non-matches, 2,364 quench-only failures, 1,031
both-failed fixtures, 159 Node-only failures, 312 output mismatches, and 211
timeouts. It completed with 16 workers and zero failed workers; the platform
audit passes with 946 prefix-classified fixtures and 963 platform-limited
non-matching results. The actionable queue remains led by TLS, HTTP, net,
filesystem, and streams.

Those differential counts are now historical: subsequent focused and source
changes invalidate the report's source/focused fingerprints. A fresh full
differential run is required before using them as the current baseline.

Fresh differential rebaseline (`target/compat/differential-parallel.json`,
2026-08-06) covers all 4,682 fixtures with 639 matches and 4,043
non-matches. It records 2,474 quench-only failures, 1,003 both-failed
fixtures, 152 Node-only failures, 303 output mismatches, and 111 timeouts;
all 16 workers completed successfully. The current queue contains 1,534
signatures, 1,009 unclassified non-matches, and 963 platform-limited
non-matches. The leading actionable signatures are HTTP (80 fixtures), TLS
(72), net (49), HTTPS (43), and fs (41).

`check-focused-stages.sh` now persists self-describing provenance in
`target/compat/focused-latest.txt`: selected stage bounds, timeout, runner and
binary digests, start/finish timestamps, elapsed time, and retry count. This
prevents a bounded green slice from being mistaken for a full focused gate.

Schema-2 differential reports now also persist SHA-256 identities for the
quench binary, differential comparator, and Node fixture runner. Together
with the existing source, fixture, focused-contract, ownership, commit, and
runtime metadata, this makes reports distinguishable even when the Git
revision is unchanged but executable or harness artifacts differ.

The parallel focused runner now accepts the same explicit stage bounds as the
serial runner and records runner/binary digests, timing, and selection
metadata. A bounded pilot over stages 1700--1707 passed 8/8 with two workers;
the corresponding serial run also passed 8/8. The measured wall times were
30 and 31 seconds respectively, so this slice does not yet demonstrate a
speedup; shared fixture setup and process/build overhead remain measurable
before parallel mode can safely become the default.

The full serial focused gate was rerun after tightening fixture cleanup and
now passes 1,786/1,786 stages with zero failures, no retries, and explicit
stage bounds `1..1808`. The cleanup change removes only scoped
`tests/node/test/.tmp.0/quench-mkdtemp-*` directories, preventing stale
temporary contents from causing order-dependent false failures.

The focused-stage telemetry now has a standalone summary command:
`node tools/summarize-focused-metrics.cjs`. On the current full gate it reports
0 retries, p50 89 ms, p95 157 ms, p99 240 ms, and a 6,620 ms maximum. This
reveals a long-tail scheduling opportunity for future adaptive or persistent
worker work without changing the verified default runner behavior.

The host `mkdtemp` allocator now retries up to 10,000 six-digit candidates,
preserving the Node suffix contract while avoiding false failures under long
focused runs. The fresh full gate completed with zero failed or retried stages.

Stage 1799 adds the public `stream.destroy()` helper and stream-prototype
destroy lifecycle with AbortError/error and close delivery. Its focused
contract passes. The upstream `test-stream-destroy.js` still fails before its
behavioral assertions in the current module-surface/test-harness path, so the
upstream result remains unresolved and is not counted as a pass.

Stage 1732 now matches Node's zlib `windowBits: 0` contract: decompression
accepts zero, compression continues to reject it, and `createUnzip()` exposes
the `zlib.Unzip` prototype. The focused contract and upstream
`test-zlib-zero-windowBits.js` both pass.

Stage 1811 adds the missing `http.globalAgent` surface. It now exposes a
keep-alive `http.Agent` with Node-compatible default socket limits, scheduling,
and inspection methods; custom Agent option propagation is covered as well.
The focused contract passes. The upstream `test-http-agent.js` still reaches
unsupported synthetic transport behavior and fails in harness callback
accounting, so transport lifecycle remains unresolved.

Stage 1812 extends the Agent contract with Node's default port/protocol,
keep-alive timing metadata, timeout-buffer metadata, and canonical `getName()`
formatting. The focused metadata contract passes; no transport capability is
claimed by this shape-only improvement.

Stage 1814 wires the default Agent reference onto `http.request()` results:
requests now expose `http.globalAgent` unless an explicit Agent is supplied,
while custom Agent identity is preserved. The focused contract passes; the
underlying network transport remains intentionally unsupported.

The full focused gate through stage 1814 now passes 1,790/1,790 with zero
retries. It records the selected-source `stage_digest` alongside runner and
binary digests, making bounded-range provenance independently auditable. A
transient stage-474 failure exposed a real precedence bug in writable
`write()` after destroy; checking `destroyed` before the ended state now returns
Node's `ERR_STREAM_DESTROYED`, and the isolated stage plus the full gate pass.

Stream lifecycle coverage also now initializes aborted state consistently,
tracks disabled readable/writable sides, and allows readable end delivery
without a data listener. Focused stages 1802 and 440 and the upstream aborted
stream fixtures pass; a later Duplex callback-order mismatch remains separate.

Stage 1813 adds the HTTPS Agent constructor and global-agent shape. The
global agent now inherits the HTTP Agent contract while exposing Node's
`defaultPort: 443` and `protocol: "https:"`; TLS request methods continue to
report `ERR_TLS_NOT_SUPPORTED`. The focused contract passes.

Stage 1815 adds the disabled-readable Duplex async-iterator contract. Node's
`for await` loop resolves immediately when `readable: false`; the shim now
matches that behavior. The focused contract and upstream
`test-stream-duplex-readable-writable.js` pass.

The remaining `test-stream-destroy.js` failure was audited against the upstream
fixture: its static readable/writable destroy blocks pass through focused
contracts, while the failing callback is in the HTTP server/request section.
The gap is therefore tracked as transport lifecycle work rather than a generic
stream-destroy regression.

Fresh differential rebaseline after the stream and Agent changes completed on
2026-08-06: 4,682 fixtures, 642 matches, and 4,040 non-matches. It records
2,469 quench-only failures, 1,004 both-failed fixtures, 152 Node-only
failures, 304 output mismatches, and 111 timeouts, with all 16 workers
successful. The current queue remains led by HTTP (80), TLS (72), net (49),
HTTPS (43), fs (41), and stream (36); platform and unclassified counts remain
963 and 1,009 respectively.

Stage 1816 fixes `fs.promises.appendFile()` with a FileHandle by forwarding
the handle's numeric descriptor to the callback filesystem primitive. The
focused FileHandle contract and upstream `test-fs-append-file.js` now pass.

Stage 1817 fixes FileHandle close state and use-after-close behavior: `close()`
sets `fd` to `-1`, and `stat()` rejects with Node-compatible `EBADF`/`fstat`.
The focused contract and upstream `test-fs-filehandle-use-after-close.js`
pass.

Stage 1821 adds Node-compatible `net.Server.listening` transitions around the
non-networking listen/close callbacks. The focused contract passes; no socket
address or TCP transport capability is implied.

Stage 1822 adds EventEmitter methods to `domain.Domain`, routes exceptions from
`domain.bind()` to the domain's `error` listeners with Node-compatible metadata,
and exposes `createDomain`/`active`. The focused contract and upstream
`test-domain-bind-timeout.js` pass.

Stage 1823 fixes `domain.run()` argument forwarding. The focused contract and
upstream `test-domain-run.js` pass; implicit EventEmitter association remains
a separate async-context feature.

Stage 1824 implements implicit domain association for EventEmitters created
inside `domain.run()`. Unhandled emitter errors are routed to the domain with
`domain`, `domainEmitter`, and `domainThrown` metadata; the focused contract
and upstream `test-domain-ee-implicit.js` pass.

Differential result records now retain explicit `node_environment_limited` and
`node_environment_reason` fields for recognizable runner/host failures such as
EPERM socket binds, ESM/CommonJS mode mismatches, missing `gc`, and missing
fixture modules. The compatibility category remains unchanged, so these
results stay visible in the queue while their measurement noise is auditable.

Parallel differential summaries now aggregate the same evidence under
`audit.node_environment_limited` and print the count at completion. A bounded
one-fixture smoke run validates the schema and summary path.

The subsequent full differential run completed all 4,682 workers but produced
537 matches and 4,145 non-matches, including 262 Node-side failures. Sample
Node-side failures are runner/environment issues (`EPERM` listen, CommonJS
fixtures loaded as ESM, and missing CLI prerequisites), so those counts are
not directly comparable to the prior 642-match baseline until the fixture
runner environment is normalized. The report and failure signatures remain
retained for triage rather than being presented as a compatibility regression.

Stage 1827 propagates active domains through `setTimeout`, `setImmediate`,
`setInterval`, and `process.nextTick`, including `process.domain` restoration.
The focused contract and upstream `test-domain-from-timer.js` and
`test-domain-nexttick.js` pass.

Stage 1826 adds domain `intercept()` error metadata and synchronous nested
`run()` error routing while preserving domain stack cleanup. The focused
contract and upstream `test-domain-intercept.js` and `test-domain-nested.js`
pass.

Stage 1829 adds the missing asynchronous Brotli convenience APIs:
`brotliCompress`, `brotliDecompress`, `createBrotliCompress`, and
`createBrotliDecompress`. The focused round-trip and transform contract
passes. The upstream Brotli and zlib-convenience fixtures still expose
separate gaps in constants/options and zstd/stream lifecycle behavior; those
remain explicitly queued rather than counted as solved by API presence alone.

The throughput review added per-stage JSONL metrics to exploratory parallel
focused runs. Serial remains authoritative because repository-relative fixture
artifacts are not isolated; parallel measurements now record duration,
outcome, retries, and isolation metadata so jobs 1/2/4/8 can be compared by
wall time and contamination rate. Differential ranking should prioritize
reproducible owned signatures by fan-out and exclude environment-limited
results from implementation priority.

Stage 1830 adds the Brotli quality, operation, mode, window, and parameter
constants to the zlib surface, using the installed Node CLI values. The
focused constants contract passes. The upstream `test-zlib-brotli.js` still
fails before exercising these semantics because the compatibility fixture
runner reports `TypeError: not a function` at its fixture setup line; this is
kept as an unresolved runner/API gap rather than reported as an upstream pass.

The setup failure was traced to the quench replacement for
`../common/fixtures`: it exposed `path` and `readKey` but omitted Node's
`readSync`. That high-fanout fixture helper is now implemented, with focused
stage 1831 covering text reads. The Brotli fixture proceeds further after this
fix and now exposes real option-validation/compression-fidelity gaps instead
of failing during setup.

The zlib Brotli path subsequently passed the complete upstream
`test-zlib-brotli.js` fixture after adding the standard `Z_*` flush constants;
stage 1832 covers the `flush`/`finishFlush` range checks. The zlib surface also
now exposes the zstd callable/constructor names as an auditable compatibility
surface, backed by a focused stage, while native zstd fidelity remains a
separate capability gap.

The next zlib convenience failure was traced to another shared Node test
helper: `common.getBufferSources()` was absent. The shim now returns the
Int8Array, Uint8Array, Uint8ClampedArray, DataView, and ArrayBuffer sources
used by upstream tests. The convenience fixture advances past setup and now
fails only on the remaining `info.engine` result contract.

The zlib callback layer now returns `{ buffer, engine }` for `options.info`
and uses the corresponding Gzip/Gunzip/Deflate/Inflate/Brotli constructor
identity; focused stage 1834 verifies the Gzip identity. The upstream
convenience fixture still reports an engine assertion in its broader matrix,
so this contract remains under investigation rather than being marked solved.

Focused stage 1835 exercises Gzip, Deflate, DeflateRaw, and Brotli
compression/decompression `info.engine` identities as a complete matrix and
passes. The upstream convenience fixture still fails its first Gzip
`info.engine` assertion despite the isolated matrix passing, indicating a
fixture-context or module-identity interaction that remains to be localized;
no upstream pass is claimed yet.

Focused stage 1836 repeats the Gzip `info.engine` check after loading the
upstream `common` shim and passes, ruling out a simple common-module mutation.
The remaining upstream-only assertion is therefore retained as an unresolved
runner/module-context interaction pending a fixture-level trace.

Focused stage 1837 additionally uses `common.mustCall` and the same repeated
payload shape as the upstream convenience fixture; it passes. A targeted
differential run confirms the upstream fixture is Node-success/quench-failure
with signature `Should get engine Gzip after gzip string with info option`,
without environment limitation. This is now a reproducible but context-
sensitive owned gap, kept in the queue for a fixture-level trace.

The subsequent full serial focused run re-established the authoritative gate:
1,813/1,813 stages pass, with zero failures and zero retries. It caught and
then verified a regression in `fs.readFile()` path validation; function-valued
paths now synchronously raise `ERR_INVALID_ARG_TYPE` as Node does. The run
recorded fresh runner, binary, and stage digests for reproducibility.

The next owned differential slice added asynchronous `crypto.generateKeyPair()`
callback behavior using normalized public/private KeyObjects; focused stage
1840 covers the callback contract and the Transform-output prerequisite (2/2
tests pass). Representative upstream crypto fixtures now reach substantive
assertions, with remaining failures attributed to missing fixture-helper data
such as `spkiExp`, not the previously absent callback API.

The callback-count mismatch was traced to missing asynchronous `deflateRaw`
and `inflateRaw` exports; only their synchronous forms existed. Both callback
wrappers are now exposed with constructor-aware behavior. The full upstream
`test-zlib-convenience-methods.js` fixture passes, and focused stage 1839
covers the raw callback roundtrip.

The synchronous zlib convenience matrix now has `info.engine` identities for
sync methods, `unzip` info results, zstd aliases, DataView byte-range input
handling, and Node-compatible missing-callback errors. Focused stage 1838 and
the info matrix pass. The upstream convenience fixture now advances to an
asynchronous callback-count mismatch, which remains a separate queued gap.
2026-08-06: The latest stale full queue was used only for ranking because its
comparator/source/focused digests no longer match the worktree. After excluding
platform-heavy TLS/HTTP/net and the requested zlib/streams clusters, the
largest reproducible owned candidate was crypto key generation (28 fixtures).
Local Node passes the async key-generation fixtures while quench previously
failed before invoking the callback (`Callback 0: expected 1 calls, got 0`).
The bounded fix in `module-surface-14.js` now implements the callback form in
terms of the normalized synchronous KeyObject pair; focused stage 1840 covers
the callback and output contract. The upstream fixtures now reach their
substantive assertions, where the current remaining failure is the fixture
helper's missing `spkiExp` RegExp rather than the absent callback API. A fresh
full differential run is required before fan-out claims are updated.

The crypto helper follow-up added the PEM RegExp exports (`spkiExp`, PKCS#1,
PKCS#8, and SEC1 variants) to the common-crypto shim and preserved requested
PEM/DER shape for encoded key-pair generation. Focused stage 1841 passes. The
DSA upstream fixture now advances beyond the missing helper/API failure and
currently reaches a later DER/signing interoperability gap.
A fresh full differential run is required before fan-out claims are updated.

The filesystem ownership subcluster now implements `chown`/`fchown` sync and
callback APIs, including Node-compatible UID/GID validation, `-1` preservation,
integer/range errors, and callback behavior. Focused stage 1847 and the
upstream type-check, ordinary fchown, and negative-one fixtures pass.

Fresh differential rebaseline `target/compat/differential-current-post-crypto.json`
completed all 4,682 fixtures with 660 exact matches and 4,022 nonmatches:
2,389 quench-only failures, 1,032 both-failed fixtures, 169 Node-only
failures, 325 output mismatches, and 107 timeouts. The report is current and
passes the platform audit (946 explicitly platform-scoped fixtures across 7
prefixes and 22 patterns). Queue ranking now shows 963 platform-limited and
994 unclassified nonmatches; the largest owned actionable groups are HTTP,
filesystem, streams, net, and TLS/HTTPS transport boundaries.

The complete post-crypto serial focused audit now passes 1,821/1,821 stages
with zero failures and zero retries. Fresh runner, binary, and full stage
selection digests were recorded; this supersedes the earlier 1,820-stage
focused baseline.

After the encrypted crypto fixes, the affected focused frontier was rerun
authoritatively from stages 1476 through 1844: 359/359 pass, zero failures,
and zero retries, with fresh binary and selected-stage digests. The earlier
1,820-stage full baseline remains valid for the unchanged lower range.

The encrypted crypto follow-up is now verified against upstream DSA, RSA, and
named-curve async key-generation fixtures. The common-crypto shim now models
the OpenSSL encrypted-key error mode, passphrase-aware signing checks, and the
encryption/decryption helper contract. All three representative encrypted
fixtures pass; broader crypto coverage remains subject to fresh differential
rebaselining.

The crypto common-helper follow-up added `assertApproximateSize` and the
encrypted-key `testSignVerify` behavior, and encoded EC private keys now use
the `EC PRIVATE KEY` PEM label when requested. Upstream
`test-crypto-keygen-async-dsa.js` and
`test-crypto-keygen-async-named-elliptic-curve.js` now pass; this closes the
previous helper/encoding failures while leaving broader cryptographic
interoperability cases queued separately.

The tooling audit added `tools/compat-decision-report.sh` and its read-only
JSON producer. It closes the immediate decision gap between a raw queue and a
safe next slice: reports now expose freshness reasons, optional resolved versus
regressed fixture counts, observed fixture-cost percentiles, and a ranked
owned/unclassified action. It also records missing evidence explicitly. The
current artifacts demonstrate why this matters: the latest 4,682-fixture
differential report has 660 exact matches but is stale against the worktree
(source and focused digests changed), while `focused-latest.txt` describes a
1,821-record full run and the current JSONL metrics contain only 820 records
from a later bounded run. The report must therefore be regenerated before
using its queue as a current baseline.

The next x2--x5 experiments remain: persistent quench workers with fresh
contexts, content-addressed fixture caching, and adaptive scheduling. Before
implementing them, collect retry/flake history, worker-level queue and startup
timings, structured failure frames, and complete cache/environment identity;
the new decision report lists these as explicit missing data rather than
inferring them from aggregate fixture durations.

The latest full serial focused audit initially caught three regressions: raw
crypto encoding shape and spoofed typed-array handling in zlib. Raw public/
private output formats now return Buffers, and zlib validates reported view
lengths before constructing a view. The corrected authoritative run passes
1,820/1,820 stages with zero failures and zero retries, recording fresh
runner, binary, and stage digests.

The filesystem URL follow-up added `common/fixtures.fileURL()` and focused
coverage for relative literal `file:` paths after `process.chdir()`. Stages
1842 and 1843 pass, including all slash-count variants. The upstream
`test-fs-whatwg-url.js` still fails in a later fixture-specific path comparison,
so the broad filesystem cluster remains split rather than over-classified.

The fs-promises appendFile investigation found and fixed eager consumption of
`Readable.from()` sources: delayed consumers now receive the original chunks.
Focused coverage also passes latin1 and large-iterable append cases. The large
case is functionally correct but currently takes about 25 seconds under
Quench; profiling isolates roughly 10 seconds to `Buffer.from()` UTF-8
encoding of a 6 MiB string in rquickjs. This is recorded as a measured
performance limitation, not an undocumented semantic failure. The upstream
appendfile fixture still exceeds the local 10-second fixture timeout, while
the focused contract passes.

After the fs validation, disposable-directory, and async-copy additions, the
full serial focused audit completed 1,829/1,829 contracts with zero failures
and zero retries (2026-08-06 18:01 UTC). This is the current authoritative
focused-contract baseline; the fresh upstream differential remains 670/4,682
exact matches and is intentionally tracked separately from focused coverage.

The latest directory work adds stages 1856 and 1857 for `Dir` async iteration,
concurrent read semantics, automatic close, and callback-style ENOTDIR. Both
focused contracts pass. The upstream `test-fs-opendir.js` now advances from
callback 9 to callback 4 after the concurrency fix; callback 4 is the
`opendir(__filename, callback)` ENOTDIR case, which passes in an isolated
contract but remains absent under the full fixture harness. This is tracked as
a public binding/integration discrepancy, not marked as resolved.

The subsequent full differential rebaseline (`target/compat/differential-final.json`,
2026-08-06 18:11 UTC) completed all 4,682 fixtures with zero worker failures:
681 exact matches, 4,001 differences, 1,030 both-failed, 167 Node-only
failures, 325 output mismatches, 2,374 Quench-only failures, and 105
timeouts. Platform coverage remains audited and passing (946 fixtures, seven
prefixes, 22 patterns); the decision report is fresh.

HTTP follow-up evidence: `test-http-max-header-size.js` now passes, including
the `spawnSync` CLI flag assertion. Sampling `test-http-max-sockets.js` still
hits the documented transport boundary: the compatibility Agent exposes
Node-shaped bookkeeping, but real socket creation is intentionally `ENOTSUP`.
This remains a platform-limited HTTP cluster, not an unclassified Agent
surface regression.

Fresh differential rebaseline (`target/compat/differential-current.json`,
2026-08-06) completed all 4,682 fixtures with zero worker failures: 670 exact
matches, 4,012 differences, 1,030 both-failed, 168 Node-only failures, 327
output mismatches, 2,387 Quench-only failures, and 100 timeouts. The platform
audit passes with 946 fixtures covered by seven platform prefixes and 22
patterns. The current decision report is fresh; its top owned action remains
the HTTP/Quench-failed cluster (78 fixtures), while four decision-data gaps
remain: trend baseline, retry history, structured failure frames/capability
probes, and worker timing.

Focused-contract verification after stages 1856-1858 completed 1,832/1,832
stages with zero failures and zero retries (2026-08-06 18:28-18:32 UTC).
The run used `QUENCH_NODE_TEST_TIMEOUT_SECONDS=60` because stage 1858 is a
deliberately expensive large-iterable append contract; elapsed time was 271s.
This confirms the new behavior and distinguishes a slow-contract budget issue
from a compatibility failure.

Stream triage then reproduced `test-stream-big-packet.js`: `Transform` lacked
the readable `.pipe()` surface, ignored backpressure for large chunks, and its
base constructor overwrote subclass `_transform()` methods. Stage 1859 now
covers `pipe(..., { end: false })`; the focused stage and the upstream fixture
both pass. This is a high-fanout stream fix; adjacent `stream.compose` and
auto-destroy fixtures remain separate failing contracts and are not claimed as
resolved.

The follow-up `stream.compose` reproduction confirms its fallback is still an
identity stub. Full duplex composition and async-generator bridging remain a
separate tracked cluster rather than being counted as covered by the `pipe`
fix.

Stage 1860 adds a synchronous `stream.compose()` contract and replaces the
identity fallback with executable synchronous stream composition plus an
experimental async-generator adapter. The focused synchronous contract passes.
The full upstream compose fixture advances past its original missing-`.on()`
failure but still fails later mixed-stream assertions; async-generator cases
remain open and are not counted as resolved.

Auto-destroy triage added explicit `autoDestroy` handling for Readable and
Writable/Transform instances, including user-provided `destroy` hooks. The
upstream `test-stream-auto-destroy.js` now reaches a lifecycle-order assertion
instead of reporting a missing destroy callback; close/end/finish ordering
remains an open contract.

Stage 1861 now audits `stream.finished()` completion on PassThrough and passes.
Its helper no longer assumes `.once()` exists, using one-shot `.on()` listeners
instead. The upstream `test-stream-finished.js` reaches a later validation
failure (`Missing expected exception`), so option/error semantics remain open.

Transform readable EOF is now also triggered from `finish`, ensuring writable
completion closes the readable side; the upstream lifecycle fixture still
exposes an ordering assertion in another case and remains open.

After the finished-listener compatibility fix, the full focused audit completed
1,835/1,835 stages with zero failures and zero retries (2026-08-06
18:46-18:48 UTC), using the 60-second stage budget for the large append
contract. An earlier run exposed stages 1211 and 1213 as regressions; both
passed after the helper was made compatible with once-only and on-only test
emitters.

The post-stage-1862 focused audit completed 1,836/1,836 stages with zero
failures and zero retries (2026-08-06 18:50-18:53 UTC), using the 60-second
per-stage budget. This is the current authoritative focused-contract baseline.

Stage 1862 adds pre-aborted `finished(..., { signal })` behavior and passes;
the helper now reports the signal reason as the completion error and handles
future abort events. Full signal cleanup and all upstream option combinations
remain subject to the broader `test-stream-finished.js` contract.

Upstream revalidation found and fixed a pre-aborted signal timing regression:
the callback is now delivered in a microtask, matching Node's cleanup-assignment
behavior. Stage 1862 still passes; the upstream fixture now returns to its
remaining later validation mismatch rather than throwing during setup.

Stage 1863 verifies all three upstream invalid-call forms for `finished()` and
passes, confirming public callback/options validation is covered. The remaining
upstream failure is therefore in a later lifecycle or internal-option case.

Fresh differential rebaseline (`target/compat/differential-parallel.json`,
2026-08-06 19:01 UTC) completed all 4,682 fixtures with zero worker failures:
703 exact matches and 3,979 differences (996 both-failed, 161 Node-only,
312 output mismatches, 2,409 Quench-only failures, and 101 timeouts). The
current decision report is `target/compat/compat-decision.json`; its largest
owned actionable queue remains HTTP/Quench-failed (78 fixtures), followed by
TLS (72), HTTPS (43), filesystem (37), and NET (35). The report records four
remaining decision-data gaps and is current against the dirty worktree.

The post-stage-1863 focused audit completed 1,837/1,837 stages with zero
failures and zero retries (2026-08-06 19:03-19:06 UTC), using the 60-second
per-stage budget. This supersedes the prior focused baseline.

Stage 1865 adds Node-compatible rejection for an `EventEmitter` falsely marked
as an ended readable while preserving real Transform streams. The focused
contract passes, and upstream `test-stream-finished.js` advances beyond that
validation case to a separate `HelloWorld` stream API failure.

Stage 1864 adds focused coverage for future abort delivery and writable-only
`finished()` options; it passes along with the existing focused suite.

The `HelloWorld` failure was traced to the repository's separate lightweight
PassThrough replacement, not the core Transform class. Its `once()` and
finish/end/close lifecycle surface is now implemented; focused stages 1866 and
1867 pass, and the upstream fixture advances to a later HTTP/lifecycle
assertion.

Stage 1868 audits preservation of the original error through
`destroy(error)` followed by `finished()`; it passes. The upstream fixture's
remaining failure occurs in a broader destroyed-stream state sequence and is
not reproduced by the isolated error contract.

Filesystem diagnostics found and fixed the async `fs.cp` filter defect: the
recursive `copyPathAsync` helper had been declared in the internal-stream
fallback scope, so the FS surface called an undefined binding. It now lives in
the FS scope, awaits filters recursively, handles dereference, and stage 1872
verifies the filtered tree. The focused diagnostic passes; the upstream fixture
remains separately harness-limited for broader fixture assertions.

Stages 1869 and 1870 cover already-destroyed streams and the distinction
between `destroy(error)` and errorless `destroy()` (`errored` remains `null` in
the latter case). Both pass; upstream `test-stream-finished.js` advances to a
separate Transform completion assertion.

Stage 1871 adds explicit Transform `finish`/readable `end` ordering coverage;
it passes after fixing `Transform.push(null)` to emit EOF. The upstream fixture
still exposes another lifecycle assertion later in its broad matrix.

The post-PassThrough focused audit completed 1,841/1,841 stages with zero
failures and zero retries (2026-08-06 19:10-19:13 UTC), using the 60-second
per-stage budget. Shared stream changes introduced no focused regressions.

The post-destroy-error focused audit completed 1,842/1,842 stages with zero
failures and zero retries (2026-08-06 19:14-19:17 UTC), using the 60-second
per-stage budget. This confirms no focused regressions through stage 1868.

The post-Transform-EOF focused audit completed 1,845/1,845 stages with zero
failures and zero retries (2026-08-06 19:19-19:22 UTC), using the 60-second
per-stage budget. This is the current focused baseline through stage 1871.

Fresh differential rebaseline after stages 1869-1871
(`target/compat/differential-parallel.json`, 2026-08-06 19:31 UTC) completed
all 4,682 fixtures with zero worker failures: 704 exact matches and 3,978
differences (996 both-failed, 160 Node-only, 312 output mismatches, 2,408
Quench-only failures, and 102 timeouts). The current decision report is
`target/compat/compat-decision.json`; its top queue remains HTTP/Quench-failed
at 78 fixtures, followed by TLS 72 and HTTPS 43. Four decision-data gaps remain
tracked.

The post-`fs.cp`-scope-fix focused audit completed 1,846/1,846 stages with
zero failures and zero retries (2026-08-06 19:33-19:37 UTC), using the
60-second per-stage budget. The filesystem change introduced no focused
regressions.

The post-PassThrough-state focused audit completed 1,847/1,847 stages with
zero failures and zero retries (2026-08-06 19:39-19:42 UTC), using the
60-second per-stage budget. This confirms the lightweight PassThrough state
change introduced no focused regressions.

The async-filter fixture investigation also verified that `.mjs` upstream
fixtures are delegated to the host Node executable by `tools/run-node-fixture.cjs`;
their generic harness result is not direct Quench execution. The focused
stage-1872 diagnostic remains the authoritative Quench-side async-filter test.

The post-`closed`-state focused audit completed 1,848/1,848 stages with zero
failures and zero retries (2026-08-06 19:48-19:52 UTC), using the 60-second
per-stage budget. This confirms the lifecycle-state change introduced no
focused regressions.

Stream queue triage found that the existing readable-slice implementation had
`drop()` and `take()` internally but never attached them to
`Readable.prototype`. Stage 1876 adds those methods and passes basic chaining
coverage; the upstream drop/take fixture still has separate async/abort
semantics outstanding.

The slice queue now carries `AbortSignal` through `drop()`/`take()` collection.
Focused stage 1877 verifies the pre-aborted signal rejects with `AbortError`.
`cargo check -p quench-node` and `git diff --check` also pass.

Stage 1878 adds the remaining high-value drop/take contracts from the upstream
fixture: async sources, chaining, coercion, finite take behavior, no-next-pull
after take completion, abort, and range/type validation. It passes in Quench;
the equivalent local Node CLI probe produces `[[2],[1],false,"AbortError"]`.

The stream/iter queue was then addressed: the prior stub only handled array
batches and exposed three async helpers. Stage 1879 now covers `from`/
`fromSync`, async and sync bytes/text/array/ArrayBuffer consumers, limits, and
pre-aborted signals. The stage passes, as do `cargo check -p quench-node` and
`git diff --check`.

Filesystem queue triage found that `Dir`/`Dirent` were substantially present
but async and promise `opendir` paths dropped options. Stage 1880 verifies
directory iteration, parent paths, closed-handle errors, ENOTDIR, and
`bufferSize` validation; the async and promise paths now validate and forward
options. The stage, cargo check, and diff check pass.

The subsequent full focused audit completed 1,853/1,853 stages with zero
failures (2026-08-06 20:00-20:03 UTC), parallel verification, one job, and a
60-second per-stage timeout. This is the current focused-contract baseline.

Fresh differential rebaseline completed all 4,682 fixtures with zero worker
failures: 682 exact matches, 4,000 differences, 1,029 both-failed, 168
Node-failed, 325 output-mismatch, 2,372 Quench-failed, and 106 timeouts. The
report is `target/compat/differential-parallel.json`. Subsequent stream/iter
triage added `tap`, `tapSync`, `pullSync`, and flush-aware pipelines in stage
1881, which passes along with cargo check and diff check.

Stage 1882 adds lazy `stream/iter.merge()` coverage for multiple sources,
strings, empty input, and pre-aborted signals. It passes, with cargo check and
diff check remaining green.

Stage 1883 covers `stream/iter.text()` encoding behavior: Latin-1 decoding,
split UTF-8 code points, BOM removal, invalid UTF-8, and encoding validation.
It passes. The installed local Node v26.5.1 does not expose `stream/iter` to
CommonJS (`MODULE_NOT_FOUND`), so this surface remains validated by the
upstream fixture contract and the repository's Quench-focused runner.

Inventory tooling now records experimental built-ins separately. The current
inventory remains 58 canonical modules / 57 registered with `node:sea` as the
only canonical gap, while explicitly recording `stream/iter` as registered and
Quench-available but unavailable in the installed host Node probe.

Stage 1884 closes direct object-source normalization for `stream/iter`: typed
arrays, `ArrayBuffer`, `DataView`, and objects exposing `toAsyncStream`. Merge,
text, and bytes contracts pass, as do cargo check and diff check.

The post-1884 full focused audit completed 1,857/1,857 stages with zero
failures (2026-08-06 20:17-20:19 UTC), parallel verification, one job, and a
60-second per-stage timeout. This is the current focused-contract baseline.

Stages 1885-1886 isolate the promise `FileHandle.read()` overloads and its
`close` event lifecycle. Both direct Quench contracts pass; the full upstream
fixture still reports a harness callback-count failure, so this cluster remains
unresolved rather than being counted as fixed.

Stage 1887 reproduces the fixture's write-then-read-then-close sequence on a
temporary file and also passes, narrowing the remaining upstream discrepancy
to its broader fixture setup/stream interaction rather than the basic read or
close contract.

Stage 1888 also passes the `createReadStream({ fd: fileHandle })` handoff and
full async stream read. The upstream callback-count discrepancy therefore does
not reproduce in the focused contracts and remains explicitly unresolved.

Stage 1889 implements and verifies `FileHandle.createReadStream()` and
`FileHandle.createWriteStream()` for promise handles, including stream
completion and byte-for-byte reads/writes. The stage, cargo check, and diff
check pass.

Stage 1890 reproduces the upstream fixture's `common/tmpdir` setup, write,
reopen, positional read, and close-event sequence; it passes. The direct
upstream runs still report callback 0 missing, so the discrepancy remains
unresolved and is not hidden by the focused evidence.

Stream/iter upstream reruns found that invalid encoding was accepted by the
synchronous text path. Stage 1883 now drives explicit Node-compatible encoding
validation for both sync and async text consumers; it passes with cargo check
and diff check. The full upstream text fixture still has an unresolved
callback-count failure after this correction.

Stage 1891 implements the missing `stream/iter.push()` source used by the
upstream tap and merge fixtures. Its write/end, async-readable, and tap pipeline
contract passes; the upstream tap fixture now passes outright. The merge
fixture still has a separate callback-count failure under investigation.

Stage 1892 adds merge cancellation and source cleanup, and stage 1893 adds the
official `toStreamable`/`toAsyncStreamable` symbols for object-like sources.
Both focused stages pass; the full merge fixture still has unresolved cleanup
error cases and is not counted as fully fixed.

Stage 1894 changes merge cleanup from suppressed `return()` errors to propagated
errors and verifies consumer-break cleanup failure. The focused contract passes;
the upstream merge fixture still reports a callback-count failure in additional
error/cleanup cases.

Stage 1895 adds primary-error tracking and `SuppressedError` composition for
merge cleanup failures. Direct source-error and normal cleanup-error contracts
pass, but the complete upstream merge fixture still has an unresolved callback
failure in its broader cancellation/cleanup matrix.

Stage 1898 reproduces the bytes/text/array/ArrayBuffer pending-read and
pending-normalization abort cases with custom reasons; it passes. The upstream
bytes fixture's callback-count failure therefore remains a separate broader
harness discrepancy, not an untested abort path.

Fresh differential rebaseline after the stream iterator and FileHandle changes
completed all 4,682 fixtures with zero worker failures: 685 exact matches and
3,997 differences (1,028 both-failed, 171 Node-failed, 325 output-mismatch,
2,372 Quench-failed, and 101 timeouts). This improves the prior 682-match
baseline by three. The current queue remains led by HTTP 78, TLS 72, HTTPS 43,
fs 36, and stream 36.

Stage 1899 directly verifies readable, writable, and transform `autoDestroy`
ordering (`finish`/`end` before `close`) and custom destroy invocation. It
passes; the upstream auto-destroy fixture still has a separate harness callback
failure and remains unresolved.

Stage 1900 verifies the static `stream.destroy()` helper for readable and
writable streams, including default `AbortError`, explicit errors, deferred
error delivery, and close state. It passes; network-dependent destroy cases
remain platform-scoped/unresolved separately.

The latest full focused audit completed 1,873/1,873 stages with zero failures
(2026-08-06 20:46-20:49 UTC), parallel verification, one job, and a 60-second
per-stage timeout. The platform coverage auditor reports no classification
failure; it marks the 20:44 differential report stale only because this newer
focused digest postdates it.

Stage 1901 fixes `NodeReadable` async iteration so it waits for future data,
end, close, and error events instead of draining only pre-buffered chunks. A
delayed-source contract passes, along with cargo check and diff check.

Stage 1902 adds `Readable.prototype.iterator(options)` with object validation
and `destroyOnReturn: false` preservation. Its focused contract passes; the
upstream async-iterator fixture now reaches its option-message assertion but
the focused contract confirms the exact Node invalid-options message. The
upstream fixture still fails later in its broader iterator matrix.

The iterator wrapper now also destroys streams on `return()` by default while
preserving streams when `destroyOnReturn: false`; stage 1902 covers both paths.
The upstream fixture advances past the options check but still encounters an
older `Stream` event-surface `TypeError` in its v1-stream section.

Stage 1903 directly reproduces that v1 `Stream` case—legacy event emission,
`Readable.prototype[Symbol.asyncIterator]`, delayed data, and end—and passes.
The generic upstream runner still reports the TypeError, so the discrepancy is
tracked as a harness/integration issue rather than hidden by the focused test.

The post-iterator full focused audit completed 1,876/1,876 stages with zero
failures (2026-08-06 21:05-21:08 UTC), parallel verification, one job, and a
60-second per-stage timeout. This is the current focused baseline.

The inventory was regenerated after the iterator changes: 58 canonical modules,
57 registered, only `node:sea` missing; 186 host globals, 218 polyfill global
assignments, and 4,682 upstream fixtures. Experimental `stream/iter` remains
explicitly registered and Quench-available while unavailable in the host Node
probe.

Decision-report classification now reports the mapped owner for owned queue
entries (for example, `Owned by workstream crypto-network-os`) instead of
incorrectly inheriting the default unclassified reason. This improves triage
evidence without changing fixture status or counts.

The latest full differential rebaseline completed all 4,682 fixtures with zero
worker failures: 685 exact matches and 3,997 differences (1,027 both-failed,
170 Node-failed, 325 output mismatches, 2,373 Quench-failed, and 102 timeouts).
The queue remains HTTP 78, TLS 72, HTTPS 43, fs 36, and stream 36; platform
coverage remains 963 explicitly limited fixtures and 992 unclassified.

Stage 1904 reproduces the `fs.promises.opendir` read, close, async-iterator,
concurrent-read, and mixed read/close cases; all pass. The upstream callback-4
failure therefore lies outside these core lifecycle paths and remains tracked
as an unresolved fixture integration discrepancy.

Stage 1905 isolates callback 4 directly: asynchronous `fs.opendir()` on a file
delivers an `ENOTDIR` error callback and passes. The full upstream fixture still
reports callback 4 missing, indicating an interaction with its preceding
assertion/setup sequence rather than the isolated async error path.

Stage 1906 reproduces the complete preceding error-validation sequence before
the callback and also passes. This narrows the upstream discrepancy further to
fixture-wide asynchronous scheduling/harness interaction; no standalone
`opendir` error-path gap is being hidden.

The coverage gate currently reports 1,904 focused-stage directories, 1,909
focused files, 4,682 upstream parallel fixtures, and deliberately leaves
`node_api_coverage` as `unmeasured`: focused-stage counts are contract gates,
not a percentage of the full Node API surface. This limitation is now explicit
in the audit output rather than presented as a misleading coverage percentage.

The post-async-iterator full focused audit completed 1,874/1,874 stages with
zero failures (2026-08-06 20:50-20:53 UTC), parallel verification, one job,
and a 60-second per-stage timeout. This is the current focused baseline.

The following full differential rebaseline completed all 4,682 fixtures with
zero worker failures: 684 exact matches, 3,998 differences, 1,027 both-failed,
171 Node-failed, 327 output mismatches, 2,373 Quench-failed, and 100 timeouts.
The async-iterator change remains covered by stage 1901 but did not yet produce
a net upstream match increase; this is retained as an explicit follow-up.

The subsequent differential rebaseline completed all 4,682 fixtures with zero
worker failures: 684 exact matches and 3,998 differences (1,027 both-failed,
171 Node-failed, 327 output-mismatch, 2,373 Quench-failed, and 100 timeouts).
The exact-match count is one below the prior 685 baseline, so the async
iterator change is retained for its direct contract but requires further
upstream differential debugging before claiming a net gain.

Stages 1896-1897 reproduce the full `FileHandle` stream fixture, including the
Node tmpdir, 100-repetition payload, and concurrent read/write promises; both
pass. The upstream fixture still rejects in its generic harness, so the
remaining discrepancy is recorded rather than treated as resolved.

The post-`stream/iter` focused audit completed 1,867/1,867 stages with zero
failures (2026-08-06 20:31-20:33 UTC), parallel verification, one job, and a
60-second per-stage timeout. This is the current focused baseline.

The post-FileHandle-stream full focused audit completed 1,862/1,862 stages
with zero failures (2026-08-06 20:23-20:26 UTC), parallel verification, one
job, and a 60-second per-stage timeout. This is the current focused baseline.

Coverage instrumentation now supplements the deliberate `node_api_coverage=
unmeasured` status with auditable inventory observations: module runtime
availability `57/58`, global assignment count `218`, and Node global surface
count `186`. These are inventory counts only, not a claim of complete Node API
coverage.

The latest focused audit completed 1,877/1,877 stages with zero failures
(2026-08-06 21:23-21:25 UTC), parallel verification, one job, and a 60-second
per-stage timeout. This is the current focused-contract baseline.

Stage 1907 adds the bounded stream observable-state contract: readable state
flags, \`readableDidRead\`, \`isDisturbed\`, \`isErrored\`, readable scheduling
flags, and writable ending/finished flags. It passes. The implementation also
keeps filesystem read-stream close ordering idempotent and preserves the
existing stream lifecycle stages.

The post-1907 authoritative serial focused audit completed 1,882/1,882
stages with zero failures, zero retries, and zero policy-covered failures
(2026-08-06 21:49-21:53 UTC; 60-second per-stage timeout). The parallel
checker showed changing failures in relative-path filesystem fixtures because
its two sides share the workspace; those same stages passed serially, so the
serial checker is the authoritative baseline for this fixture set.

The decision-report tooling now fails closed for action selection when the
differential report, focused summary, and focused metrics JSONL do not share a
valid run identity. It records `focused_join` checks for metrics record count,
current commit, focused digest, and report commit; an invalid join still emits
diagnostics but produces no actionable queue. This prevents stale focused data
from steering implementation work.

The differential comparator now records structured per-side error evidence for
non-matching fixtures: error name, code, first message line, source location,
and an inferred phase (`callback`, `promise`, `cleanup`, `timeout`, or
`process`). A one-fixture smoke run verified the schema without changing match
classification. This is intended to split broad signatures into actionable
failure clusters on the next full rebaseline.

The subsequent full differential rebaseline completed all 4,682 fixtures with
zero worker failures (2026-08-06 21:34-21:41 UTC): 696 exact matches, 3,986
differences, 1,027 both-failed, 171 Node-failed, 325 output-mismatch, 2,362
Quench-failed, and 101 timeouts; 575 results were marked
`node_environment_limited`. Structured triage identified the largest explicit
owned/platform clusters as HTTP/2 unsupported server setup (200), QUIC flag
environment output (220 combined), VFS internal-module absence (93), HTTP
harness callback failures (89), and TLS/HTTPS unsupported server setup (72/43).
The report is retained as the latest rebaseline, while current freshness checks
must pass before its queue is used for implementation decisions.

The focused-stage runner now reports 1,879/1,879 passing contracts with zero
failures (parallel mode, one job); this supersedes the earlier 1,877 baseline
after two additional stages appeared in the worktree. The focused result is
valid as a contract gate, but the differential report is intentionally stale
against the newer focused/source digests until rebaselined again.

Stage 1909 fixes the owned HTTP server close lifecycle. `http.Server.close()`
now clears the listening state immediately, emits `close` asynchronously, and
invokes its callback from that close event; closing a non-listening server
reports `ERR_SERVER_NOT_RUNNING`. The focused contract also verifies callback
ordering, callback `this`, and the chainable return value. The upstream
`test-http-server-async-dispose.js` and `test-http-write-empty-string.js`
fixtures pass; `test-http-server-close-all.js` still stops at the native socket
boundary and remains unresolved rather than being attributed to this lifecycle
fix.

The post-lifecycle serial focused audit completed **1,882/1,882** stages with
zero failures (2026-08-06 21:48-21:51 UTC). A parallel rerun exposed only
stages 423 and 424, which both pass individually and serially; these are shared
temporary-file contention in the parallel runner, not compatibility failures.

The post-HTTP-lifecycle and stream-observable-state differential rebaseline
completed all 4,682 fixtures with zero worker failures (2026-08-06 21:52-21:59
UTC): **700 exact matches**, 3,982 differences, 1,025 both-failed, 173
Node-failed, 328 output-mismatch, 2,355 Quench-failed, and 101 timeouts; 574
results were marked `node_environment_limited`. This is a +4 exact-match gain
over the preceding 696-match baseline. Freshness and platform coverage audits
both pass; the largest remaining explicit platform cluster is HTTP/2 server
support (200), while the largest owned callback cluster is HTTP (92).

Stage 1910 begins separating the outgoing HTTP `ClientRequest` lifecycle from
the server-side `IncomingMessage`: `finish`, `finished`, and
`writableFinished` are tracked independently, and server request `end` is
emitted on a distinct object. The focused contract passes and verifies
`client-finish` before `server-end`. Upstream outgoing-message fixtures still
expose additional response-side writable-state and callback gaps, so this is
recorded as a bounded partial fix rather than a resolved HTTP cluster.

The follow-up response-state pass adds `ServerResponse` writable flags and
`finish` transitions, and invokes `write()` callbacks for empty and non-empty
chunks. The upstream `test-http-outgoing-finish-writable.js` and
`test-http-outgoing-writableFinished.js` fixtures now pass. Constructor-level
`ServerResponse`/`assignSocket` coverage and close-order fixtures remain
separate unresolved API gaps.

Stage 1911 adds `http.ServerResponse` construction and `assignSocket()` with
socket ownership, duplicate-assignment validation, socket event delivery, and
write/end callback support. The focused stage and upstream
`test-http-outgoing-message-write-callback.js` now pass. The upstream finished
stream teardown and request/response close-order fixtures still expose
independent lifecycle gaps and remain explicitly unresolved.

The focused event-order probe reproduces Node’s `response-close` before
`stream.finished()` completion, and the upstream
`test-http-outgoing-finished.js` fixture now passes. The broader
`test-http-req-res-close.js` fixture still exposes a missing socket/request
surface (`TypeError: not a function`) and remains separately tracked.

Stage 1913 isolated and fixed the HTTP close-order defect: for requests without
a data listener, the observed sequence is now `res-finish`, `res-close`,
`req-end`, `req-close`, matching local Node; data-consuming requests retain
their earlier request-before-response close ordering. The upstream
`test-http-req-res-close.js` fixture now passes.

Stage 1915 corrected callback validation metadata for `fs.stat` and `fs.lstat`:
invalid callbacks now throw `TypeError` with `ERR_INVALID_ARG_TYPE`. The focused
contract and upstream `test-fs-makeStatsCallback.js` both pass.

The subsequent full differential rebaseline completed all 4,682 fixtures with
zero worker failures (2026-08-06 22:16-22:23 UTC): **704 exact matches**, 3,978
differences, 1,025 both-failed, 172 Node-failed, 326 output-mismatch, 2,344
Quench-failed, and 111 timeouts; 572 results were marked
`node_environment_limited`. This is a further +4 exact-match gain over the
700-match baseline. Freshness and platform audits pass; the HTTP callback
cluster declined from 92 to 90 fixtures.

Stages 1916-1917 add focused coverage for bounded stream combinator execution
and the `Readable.map()` surface. The surface contract passes; upstream
combinator fixtures still fail in deeper callback/scheduling cases and remain
explicit follow-up work. The full differential report predates these focused
stages and must be rebaselined before measuring their effect.

The post-stream-combinator serial focused audit completed **1,889/1,889**
stages with zero failures (2026-08-06 22:39-22:42 UTC), no retries, and zero
covered policy failures. This restores the 100% focused-contract gate after
the transient stage-1910 regression was reconciled with the corrected HTTP
event lifecycle.

The post-focused-gate full differential rebaseline completed all 4,682 fixtures
with zero worker failures (2026-08-06 22:43-22:50 UTC): **708 exact matches**,
3,974 differences, 1,026 both-failed, 172 Node-failed, 328 output-mismatch,
2,342 Quench-failed, and 106 timeouts; 573 results were marked
`node_environment_limited`. This is a +4 gain over the 704-match baseline.
Freshness and platform audits pass. Stream combinator fixtures remain explicit
follow-up failures (`map`, `filter`, and `forEach`), while the focused gate is
still 1,889/1,889.

The latest full differential rebaseline completed all 4,682 fixtures with zero
worker failures (2026-08-06 22:53-23:02 UTC): **704 exact matches**, 3,978
differences, 1,025 both-failed, 172 Node-failed, 326 output-mismatch, 2,340
Quench-failed, and 115 timeouts; 568 results were marked
`node_environment_limited`. Freshness and platform audits pass. This is a
regression of four exact matches versus the prior 708 baseline, coinciding
with the stream-combinator changes; those changes require further isolation
before they are retained as a net compatibility improvement.

Differential triage now extracts callback index/expected/actual counts from
harness failures and includes structured error evidence in decision grouping.
The fresh report completed 2026-08-06 23:06-23:16 UTC with the current
comparator: 4,682 fixtures, 701 exact matches, 3,981 differences, 1,027
both-failed, 173 Node-failed, 327 output-mismatch, 2,339 Quench-failed, and
115 timeouts; 573 results were marked `node_environment_limited`, with zero
worker failures. Freshness and platform audits pass, and the focused join is
valid. The exact-match count moved by seven across recent full runs (701-708),
so single-run deltas are not decision-quality evidence for timeout-sensitive
clusters.

The throughput instrumentation now makes the missing decision data explicit:
retry history, structured per-fixture capability probes, and worker timing are
still absent. The next speed-up is therefore to add a bounded repeated-run
cluster benchmark and capability probes, then prioritize only clusters that
improve across repetitions and are not already covered by platform ownership.
The current actionable queue is TLS/quench-failed (72 fixtures), followed by
the previously observed HTTP/FS/net/stream callback clusters; TLS remains
platform-constrained and must not outrank implementable clusters merely due to
raw count.

Stage 1918/1919 implement the missing `fs.promises.FileHandle.read()`
positional and options-object forms, returning the Node-compatible
`{ bytesRead, buffer }` result and preserving buffer identity. Both focused
contracts pass. The upstream `test-fs-promises-file-handle-read.js` still
fails at its later `FileHandle`-backed `createReadStream` async-iteration
contract, so this cluster is not counted as an upstream win yet; that remaining
failure is now isolated from the primitive read implementation and belongs to
the file-handle/read-stream integration follow-up.

Stages 1922-1923 fix and cover a separate stream lifecycle primitive: flowing
`Readable.resume()` now drains buffered chunks even when no `data` listener is
present, allowing queued `end` and `autoDestroy` transitions to occur. The
focused readable and transform contracts pass. The upstream
`test-stream-auto-destroy.js` still reports an uncalled later lifecycle
callback, so the broader pipe/error branch remains an explicit follow-up and
this change is not counted as an upstream match until that fixture passes.

The stream error branch is now resolved. Callback telemetry showed the missing
hook was the 11th callback (zero-based index 10): the readable destination in
the pipe/error case. `Readable` and `Writable` with `autoDestroy` now destroy
themselves after an already-emitted error without re-emitting that error;
custom `destroy()` hooks receive the Node-compatible null error and `close`
follows. Stages 1925-1926 pass, and the upstream
`test-stream-auto-destroy.js` now passes exactly.

The serial focused audit then caught stages 426 and 443. Stage 443 exposed
append-mode truncation: `createWriteStream()` now opens with the requested
flags before appending. Stage 426 exposed an invalid focused expectation
(`Buffer.byteLength("write stream text")` is 17 in local Node, not 16); the
contract was corrected and `bytesWritten` is now derived from the actual
payload. The refreshed gate passes **1,898/1,898** stages with zero failures,
zero retries, and zero covered policy failures (2026-08-06 23:35-23:38 UTC).

Stage 1929 adds a focused async-filter `fs.cp` contract and passes. The
upstream `test-fs-cp-async-async-filter-function.mjs` still fails without a
diagnostic payload, so the general primitive is covered locally but not yet
counted as an upstream compatibility win; the fixture-specific ESM/common
filesystem integration remains to be isolated.

A minimal stage-1930 ESM probe confirmed the gap is earlier than `fs.cp`: even
`node:assert`, `node:fs`, and `node:timers/promises` imports fail before module
body execution. The Rust loader currently reports only a generic QuickJS
`Exception`, so this is documented as an ESM builtin-loader gap rather than
misattributed to filesystem copying.

The ESM loader gap is resolved: `timers/promises` is now registered with its
named exports (`setTimeout`, `setImmediate`, `setInterval`, and `scheduler`).
Stage 1930 passes, and the upstream
`test-fs-cp-async-async-filter-function.mjs` now passes exactly. The failure
was loader metadata, not `fs.cp` filter semantics.

The post-loader-fix focused audit passes **1,900/1,900** stages with zero
failures, zero retries, and zero covered policy failures (2026-08-06
23:43-23:46 UTC).

Stage 1932 fixes the high-fanout stream pipe lifecycle: `Readable.pipe()` and
the base `Stream.pipe()` now emit `pipe` on the destination with the source,
matching local Node. The focused contract and upstream
`test-stream-events-prepend.js` both pass.

The refreshed focused audit including stage 1932 passes **1,901/1,901**
stages with zero failures, zero retries, and zero covered policy failures
(2026-08-06 23:57-2026-08-07 00:01 UTC).

Stage 1933 adds the missing `fs.ReadStream.close(callback)` API with
idempotent close state and per-call callback delivery, matching Node's
`internal/fs/streams.js`. The focused contract and upstream
`test-fs-read-stream-double-close.js` pass.

The focused audit including stage 1933 passes **1,902/1,902** stages with
zero failures, zero retries, and zero covered policy failures (2026-08-07
00:02-00:05 UTC).

The neighboring file-URL `fs.cp` fixture exposed a second ESM metadata gap:
`node:url` was not registered with `pathToFileURL`/`fileURLToPath` exports.
Those URL exports are now registered; stage 1934 and upstream
`test-fs-cp-async-file-url.mjs` pass.

The focused audit including stage 1934 passes **1,903/1,903** stages with
zero failures, zero retries, and zero covered policy failures (2026-08-07
00:07-00:10 UTC).

Stage 1935 adds `Writable.setDefaultEncoding()` with Node-compatible chaining
and invalid-encoding validation. The focused contract and upstream
`test-fs-read-stream-encoding.js` pass.

The focused audit including stage 1935 passes **1,904/1,904** stages with
zero failures, zero retries, and zero covered policy failures (2026-08-07
00:42-00:45 UTC).

The next ESM loader gap was `node:net` named exports. Registering the supported
net surface (`createServer`, `connect`, `Socket`, address helpers, and related
constructors) makes stage 1936 pass and restores upstream
`test-fs-cp-async-socket.mjs` exactly.

The focused audit including stage 1936 passes **1,905/1,905** stages with
zero failures, zero retries, and zero covered policy failures (2026-08-07
00:47-00:50 UTC).

Further isolation of `test-fs-promises-file-handle-read.js` with the upstream
fixture paths shows the read and stream data operations complete; the missing
callback is specifically a `common.mustCall()` listener attached to
`FileHandle.close()`. A normal listener receives the event, so the remaining
gap is the FileHandle/EventEmitter integration path. The temporary diagnostic
stage was removed rather than counted as focused coverage.

The post-loader full differential rebaseline completed all 4,682 fixtures with
zero worker failures (2026-08-06 23:47-23:55 UTC): **708 exact matches**, 3,974
differences, 1,024 both-failed, 172 Node-failed, 338 output-mismatch, 2,331
Quench-failed, and 109 timeouts; 574 results were marked
`node_environment_limited`. Freshness, platform coverage, and focused-join
audits pass. This is +7 exact matches over the previous 701 baseline; the
remaining variance still warrants repeated cluster measurements before
attributing every delta to a code change.

After the `node:url` ESM exports and `ReadStream.close()` fixes, the next full
differential rebaseline completed all 4,682 fixtures with zero worker failures
(2026-08-07 00:10-00:18 UTC): **716 exact matches**, 3,966 differences, 1,024
both-failed, 173 Node-failed, 341 output-mismatch, 2,322 Quench-failed, and
106 timeouts; 575 results were marked `node_environment_limited`. Freshness,
platform coverage, and focused-join audits pass. This is +8 exact matches over
the 708 baseline.

After the `node:net` ESM exports and focused stream encoding/read-stream
changes, the full differential rebaseline completed all 4,682 fixtures with
zero worker failures (2026-08-07 00:50-00:58 UTC): **718 exact matches**,
3,964 differences, 1,026 both-failed, 172 Node-failed, 340 output-mismatch,
2,320 Quench-failed, and 106 timeouts; 575 results were marked
`node_environment_limited`. Freshness, platform coverage, and focused-join
audits pass. This is +2 exact matches over the 716 baseline.

Stage 1940 isolated the remaining `fs/promises` FileHandle read fixture gap:
null and explicitly undefined `length` values were passed through as zero-ish
host read lengths instead of defaulting to the available buffer length. Both
positional and options-object reads now normalize those values like Node. The
focused variant matrix and upstream `test-fs-promises-file-handle-read.js`
pass; the earlier FileHandle close-listener diagnosis was superseded by this
read-path finding.

Stage 1941 adds `net.Socket.bufferSize` accounting: queued writes report their
byte count and the value returns to zero before `finish`. The focused contract
passes. Preserving the network shim's address implementation through the
post-bootstrap net-surface wrapper, together with the listen callback receiver,
also restores the upstream `test-net-buffersize.js` fixture.

The post-network full differential rebaseline covered the expanded 8,457-fixture
corpus with zero worker failures: **1,337 exact matches**, 7,120 differences,
2,635 both-failed, 431 Node-failed, 487 output-mismatch, 3,407 Quench-failed,
and 160 timeouts; 1,068 results were marked `node_environment_limited`.
The focused join is valid. The expanded corpus supersedes the earlier 4,682
fixture baseline for subsequent queue decisions.

The preceding 8,457-fixture measurement was invalid for the canonical
parallel corpus because it was run against `tests/node/test` rather than
`tests/node/test/parallel`; it is retained only as a tooling-diagnostic event.
The corrected parallel rebaseline completed 4,682 fixtures with zero worker
failures (2026-08-07 02:01-02:08 UTC): **727 exact matches**, 3,955
differences, 1,021 both-failed, 174 Node-failed, 343 output-mismatch, 2,308
Quench-failed, and 109 timeouts; 574 were marked `node_environment_limited`.
Freshness, platform coverage, and focused-join audits pass.

Stage 1947 fixes lazy EventEmitter initialization for stream-like objects that
inherit `EventEmitter.prototype` without running its constructor. The focused
contract and upstream `test-repl-let-process.js` pass; this removes a shared
REPL/stream failure rather than adding a REPL-specific workaround.

Stage 1949 adds the missing `REPLServer.write()` surface, including evaluation
of input and `.exit` handling. Its focused contract and upstream
`test-repl-async-iife.js` pass.

Stage 1951 fixes `util.inspect()` argument normalization for Node's legacy
overloads, including `null` options and `(showHidden, depth, colors)` calls.
The focused contract passes. This is separate from the explicitly limited
REPL autoload/inspection pipeline.

Stage 1953 adds bounded Node-style rendering for plain objects and
function-valued properties instead of JSON-dropping functions. Its focused
contract passes. The upstream util-inspect fixture now reaches one remaining
exotic generator/prototype/tag rendering difference, which remains explicitly
tracked rather than hidden behind a heuristic.

Native CLI comparison confirms `test-repl-autolibs.mjs` additionally depends
on Node's full REPL autoload/inspection pipeline (including exact `util.inspect`
rendering and global identity). The minimal REPL intentionally does not claim
that surface; the fixture remains classified as an explicit REPL limitation.

The `net-after-close` cluster is now covered by the in-process server shim:
listening servers receive synthetic connections, `Socket.end()` clears
`_handle` and emits the expected close lifecycle, and post-close methods such
as `setNoDelay()`, `setKeepAlive()`, `address()`, and `bufferSize` are safe to
query. Focused stage 1944 and upstream `test-net-after-close.js` pass.
The post-formatter canonical parallel rebaseline completed all 4,682 fixtures
with zero worker failures: **726 exact matches**, 3,956 differences, 1,022
both-failed, 176 Node-failed, 345 output-mismatch, 2,303 Quench-failed, and
110 timeouts; 573 were marked `node_environment_limited`. Freshness, platform
coverage, and focused-join audits pass. The exact-match count is one below the
previous 727 baseline, so the formatter work improves focused behavior but does
not yet improve aggregate upstream parity.
2026-08-07: The stream-promises differential slice found that the promise
`finished()` shim only observed `end`, so writable-only streams could finish
their writes while the returned promise remained pending. It now tracks the
readable `end` and writable `finish` sides independently, honoring explicit
`readable`/`writable` options. Focused stages 1896-1897 and the complete
upstream `test-fs-promises-file-handle-stream.js` fixture now pass.
2026-08-07: Extended the stream-promises audit to `pipeline()`. The shim now
tracks destination `finish` as well as `end`, and propagates source and
destination errors; focused stage 525 remains green. The upstream
`test-stream-promises.js` fixture still has one missing callback, indicating
remaining parity work in the full `finished()` lifecycle/cleanup contract.
2026-08-07: Platform-fixture tracing found that `common/crypto` lacked the
upstream `hasOpenSSL()` capability helper; it is now exposed. The injected
`common.skip()` helper also prints Node's skip marker and sets the existing
forced-exit flag, but the evaluator currently continues the same script after
`process.exit()`. Consequently conditional fixtures can still execute code
after a skip. This is documented as a host evaluator control-flow gap rather
than counted as a compatibility pass.
2026-08-07: The evaluator now suppresses the immediate JavaScript exception
when `process.exit()` has marked a forced exit, matching the intended host
control-flow boundary. The conditional Argon2 fixture still continues into
its post-skip assertion, so the process object/forced-exit flag propagation
needs a direct runtime probe before this is counted as fixed.
2026-08-07: A focused process-exit probe confirmed `process.exit` is a
function and sets both the process-local and global forced-exit flags. An
attempt to interrupt evaluation by throwing a sentinel was reverted because
the host still surfaced the sentinel as a test exception. The remaining work
is to make the evaluator consume that sentinel/flag at the exact QuickJS
evaluation boundary without converting a normal skip into a failure.
2026-08-07: Fixed `fs.opendirSync()` invalid encoding validation. Node throws
`ERR_INVALID_ARG_VALUE` with the exact `invalid encoding` message for values
such as `"no"`; Quench previously accepted the option. The focused probe and
upstream `test-fs-internal-assertencoding.js` now pass.
2026-08-07: Resolved the platform-skip control-flow gap. `common.skip()` now
uses a process-exit sentinel after setting the forced-exit flags, and the
QuickJS entry evaluator consumes evaluation errors when that flag is set.
This prevents skipped fixtures from continuing into unsupported assertions.
The upstream `test-crypto-argon2-unsupported.js` fixture now exits cleanly
with Node's skip marker; focused stage 1154 remains green.
2026-08-07: Aligned `common.skipIfInspectorDisabled()` with Quench's
inspector capability boundary. It now routes through the verified skip
sentinel instead of allowing interactive REPL fixtures to run against an
unsupported inspector. Upstream `test-repl-array-prototype-tempering.js` now
exits cleanly with a skip marker; focused common-capability coverage remains
green.
2026-08-07: The inspector skip-path fix also resolves the dependent REPL
fixtures `test-repl-custom-eval.js` and `test-repl-let-process.js`; both now
exit cleanly with the expected V8-inspector-disabled marker. This confirms the
sentinel is reusable across synchronous and asynchronous fixture setup.
2026-08-07: Batch validation of inspector-gated fixtures confirms direct
`skipIfInspectorDisabled()` cases such as `test-repl-colors.js` and
`test-debugger-inspect-help.js` now pass with clean skips. Remaining failures
in `test-repl-syntax-error-handling.js` and `test-repl-preview-newlines.mjs`
exercise REPL behavior without a top-level skip, while `test-inspector-open.js`
requires child-process inspector IPC; these remain explicitly classified
platform/runtime gaps.
2026-08-07: The refreshed differential queue's FileHandle write slice exposed
an input-validation gap: `FileHandle.write(null)` and `.write(undefined)`
escaped through range handling and produced an untyped internal exception.
They now reject with `ERR_INVALID_ARG_TYPE` and the standard buffer argument
message. The full concurrent upstream write fixture still reports a separate
callback/assertion gap, so this remains a partial fix.
2026-08-07: The crypto-GCM queue slice was traced to the reachable
`createCipheriv` fallback: explicit `authTagLength` was stored nowhere, so a
12-byte tag was accepted for a 14-byte contract. The cipher state now retains
the requested tag length and `setAuthTag()` throws Node's
`ERR_CRYPTO_INVALID_AUTH_TAG` with the matching message. The focused runtime
probe and upstream `test-crypto-gcm-explicit-short-tag.js` pass.
2026-08-07: Completed the adjacent implicit GCM tag-length contract. Without
an explicit `authTagLength`, `setAuthTag()` now requires the default 16-byte
tag; with an explicit option it requires that exact length. Upstream
`test-crypto-gcm-implicit-short-tag.js` and the explicit short-tag fixture
both pass after rebuilding the runner binary.
2026-08-07: Runtime tracing found the cipher authentication fallback did not
track whether the selected algorithm supports AAD. Cipher state now records
that capability and rejects `setAAD()` with `Invalid state` for CBC and other
non-authenticated modes. The direct runtime contract passes; the broad
authenticated fixture still reports a separate lifecycle mismatch.
2026-08-07: Post-AAD-state verification passes focused stage 1953 and the
upstream authenticated-stream fixture (with its expected unsupported-CCM
skips). The remaining broad authenticated fixture mismatch is isolated to a
different lifecycle path.
2026-08-07: Extended crypto validation to GCM `authTagLength` constructor
options and corrected non-authenticated `getAuthTag()` state handling. The
authenticated fixture now advances past state/argument assertions to a later
cryptographic-output mismatch, confirming the validation fixes are active;
the remaining output difference requires real cipher implementation parity.
2026-08-07: Instrumentation audit found the canonical differential report's
source digest is stale after the latest crypto changes. The report still has
4,682 fixture rows, but its queue is not used for new implementation choices
until `diff-node-quench-parallel.sh` is rerun. Focused metrics remain valid
independently.
2026-08-07: The fresh queue's filesystem `mkdtempDisposable` slice revealed
that `fs/promises` is cloned at the final module-loader boundary. The
disposable API was added there as well as to the base promises surface, but
the upstream fixture still reports a separate completion gap; this remains
unresolved pending a focused lifecycle trace.
2026-08-07: Focused tracing corrected `fs/promises.mkdtempDisposable()` at the
final cloned module boundary: its implementation must call the synchronous
mkdtemp primitive because the cloned `result.mkdtemp` is callback-shaped.
Basic creation, async disposal, and idempotent removal now pass in a focused
probe. The complete upstream fixture still exposes a separate concurrent
lifecycle gap.
2026-08-07: Directly reproduced the upstream `mkdtempDisposable` CWD branch:
relative prefixes, removal after changing into another disposable directory,
and restoration of the original CWD all pass. The unresolved upstream callback
gap therefore lies outside the basic disposable/path implementation.
2026-08-07: HKDF queue triage added synchronous validation for digest,
ikm/salt/info sources, output length, and invalid digests; the callback API
now validates before scheduling completion, matching Node's synchronous throw
behavior. Focused stages 901 and 905 pass. The upstream fixture advances to a
later invalid-digest assertion mismatch, so this remains a partial cluster.
