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
- Item 2: improved further. `net.Server.close()` now tracks accepted sockets
  and completes its close callback/event after the tracked set drains. The
  authoritative `test-net-server-close.js` passes; native transport and
  broader remote-address lifecycle cases remain separate.
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

Stage 2442's differential trace found one missing general stream contract:
Node's `Readable.pipe()` starts flowing by calling `resume()`, while quench
only installed the data/end listeners. `pipe()` now resumes the readable, and
the focused trace plus existing backpressure/drop stages pass. The authoritative
backpressure fixture still reports one `_read()` call instead of eleven, so
downstream drain/read-demand scheduling remains unresolved.

The host-backed timer prototype is now active for verification: stage 2444
passes the delayed timer ordering contract, and stages 2047, 2069, 2081, 2104,
2440, and 2442 remain green. The authoritative backpressure fixture still
reports callback 1/11, and the full watch-promises fixture still times out;
these are retained as the next scheduler/stream interactions.

Stage 2445 confirms the host timer interleave is effective but incomplete: the
large backpressure trace reaches 18 writes and 3 reads, up from 1 write before
interleaving. The remaining 3/11 readable-demand mismatch is now isolated from
timer starvation and is the next stream-state target.

The upstream backpressure guard also exposed a missing public state contract:
`_readableState.length` was absent. It now reflects the live buffered-chunk
length. The exact fixture advances from an assertion exception to a
deterministic 3/11 `_read()` mismatch; the failing diagnostic stage was
removed, while the passing large trace remains as evidence.

Stage 2443 independently verifies `Readable.from(asyncGenerator).take(1)`
resolves `toArray()` without waiting for the generator's next promise, and the
upstream generator remains unadvanced until explicitly released. The full
`test-stream-drop-take.js` fixture still misses its combined callback, so the
remaining difference is an aggregate promise/finally scheduling interaction,
not basic take cancellation.

Post-`42edaf10e` regression checkpoint: the Ajv, debug, `ms`, and Prettier
application stages pass, along with stream stages 2343, 2370, 2372, 2440, and 2442. This confirms the `pipe()` auto-resume change did not regress the
maintained application or focused stream coverage; the upstream backpressure
fixture remains the authoritative failure.

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

Stage 2373 adds the missing root-permission contract: `fs.accessSync()` and
callback-style `fs.access()` now allow `W_OK` on a mode-0444 file when
`process.getuid() === 0`, matching Node's superuser behavior. The aggregate
`test-fs-access.js` fixture still reports `Callback 7` missing, so its
remaining callback is separate from the root permission decision.
Stage 2374 reproduces the upstream `assert.rejects()` predicate plus chained
`.then()` and passes, including the async-access stack assertion. The full
fixture's `Callback 7` therefore depends on its larger callback/credential
sequence, not the isolated promise assertion chain; adding the preceding
missing-path rejection observer to the stage also passes.

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

Stage 2375 fixes and verifies VFS permission semantics: virtual entries retain
`chmodSync()` mode bits, `accessSync()` enforces `R_OK`/`W_OK`/`X_OK`, and the
promise path reports `EACCES`. The authoritative
`test-vfs-access-modes.js` and `test-vfs-fs-accessSync.js` fixtures now pass.
Stage 2376 fixes path-based VFS `Buffer`/`Uint8Array` writes; the focused
regression passes and the upstream `test-vfs-fs-writeFileSync.js` now advances
past its direct buffer assertion. Its remaining failure is descriptor offset
semantics (`" world"` instead of `"hello world"`), which is tracked separately.
Stage 2377 verifies in-memory VFS descriptor writes now honor the current file
offset and preserve prior content. The upstream write fixture reaches a later
real-provider `/a.txt` open failure after this virtual-descriptor behavior,
which remains a separate provider boundary.
The adjacent real-provider checks confirm that boundary is broader than one
write assertion: `test-vfs-fs-openSync.js` passes, while
`test-vfs-real-provider-handle.js`, `test-vfs-real-provider-promises.js`, and
the mounted portions of `test-vfs-fs-readFileSync.js`/
`test-vfs-fs-writeFileSync.js` still fail because wrapped JS descriptors do not
reach a native fd-backed read/write/close seam.
That native seam is now implemented for real-provider synchronous descriptors:
open, read-from-current-offset, write-at-current-offset, and close use host
fd operations. `test-vfs-fs-writeFileSync.js`,
`test-vfs-fs-readFileSync.js`, and `test-vfs-fs-openSync.js` pass, including
write-after-rename inode behavior. Real-provider handle/promises fixtures still
need their async handle methods wired to the same native descriptor state.
Stage 2382 adds and verifies the first async handle slice: real-provider
`provider.open()` now exposes native-fd-backed `readFileSync()`, `readFile()`,
and idempotent close behavior. The upstream handle fixture still fails before
this slice in its synchronous `getVirtualFd()` path; positioned read/write,
stat, and truncate methods remain queued.
Stage 2383 registers native descriptors with `internal/vfs/fd` and verifies
positioned `readSync()` and `writeSync()` on a real-provider handle. The
focused stage passes. Stat and the upstream fixture's remaining `read()`,
`write()`, `writeFile*()`, `truncate*()`, and async edge cases remain queued.
Stage 2384 reduces `test-vfs-real-provider-promises.js` to its complete
operation sequence and verifies real-provider promise write/read, stat/lstat,
access success and `ENOENT`, recursive mkdir/readdir/rmdir, rename/unlink,
copyFile, and missing-file provider open. The synchronous-reduction probe
passes, but a trustworthy event-loop-held probe stops at the first awaited
`promises.readFile()` after `promises.writeFile()`. The authoritative fixture
still reports an opaque `Callback 0` exception, so the full promises surface
remains unresolved.
The real-provider handle implementation now also supports positioned async
`read()`/`write()`, descriptor-backed `writeFileSync()`/`writeFile()`, and
`truncateSync()`/`truncate()` through native `pread`/`pwrite`/`ftruncate`.
Stage 2383 passes with the full focused sequence. The authoritative handle
fixture still reports `Callback 0`; its instrumented `fs.fstatSync`/`fs.fstat`
calls do not observe the internal filesystem object used by this bootstrap,
so public-observer identity remains the next compatibility boundary.
Stage 2383 also confirms a public `fs.fstatSync` monkey-patch observes the
descriptor-backed `readFileSync()` call. The remaining upstream callback
failure is therefore narrowed to its combined async metadata instrumentation
and later handle lifecycle sequence.
Stage 2385 reproduces the upstream zero-stat read section. Both sync and async
public `fstat` hooks are reached while the descriptor-backed read still returns
the complete file. The runtime's public async `fstat` implementation performs
an additional internal invocation, so its function-entry count differs from
Node's single `common.mustCall` callback contract; this remains an event-loop
callback identity issue rather than a missing read-data path.
The native whole-file read was then changed from an unbounded `read()` loop to
size-bounded `fstat`/`pread`, eliminating a real-provider hang after writes.
The authoritative handle fixture now advances from `Callback 0` to
`Callback 1`; the remaining callback is still the zero-stat metadata contract.
Stage 2386 adds a non-blocking public `fs.fstat` observer to real-provider
`FileHandle.readFile()` while keeping descriptor reads independent of provider
metadata. Its focused handle-surface stage passes, and the authoritative
`test-vfs-real-provider-handle.js` now passes completely.
Stage 2388 adds host-native canonicalization for real-provider `realpathSync()`
and verifies string and buffer results. The authoritative
`test-vfs-real-provider.js` now passes completely; real-provider symlink and
watch fixtures remain separate failures.
Stage 2389 adds native `readlinkSync()` support for real providers and verifies
relative targets plus absolute in-root target translation. The synchronous
symlink path is now covered; the authoritative symlink fixture still reports
an async callback failure after these operations.
Stage 2390 adds real-provider `watch()`, `watchFile()`/`unwatchFile()`, and
`promises.watch()` iterator surfaces. The focused watch-surface stage and
authoritative `test-vfs-real-provider-watch.js` now pass.
Stage 2392 verifies that real-provider `symlinkSync()` rejects an absolute
target outside the provider root with `EACCES`. The upstream symlink fixture
still fails in its async assertion sequence, so async rejection and promise
readlink/realpath ordering remain separate work.
Stage 2393 adds the missing `internal/vfs/router` module and verifies mount
containment, relative-path conversion, and absolute-path detection. The
authoritative `test-vfs-router.js` now passes completely.
Stage 2394 adds the missing `internal/vfs/file_handle` `VirtualFileHandle`
base class, including stub error contracts, metadata no-ops, close state, and
async disposal. The focused stage and authoritative
`test-vfs-virtual-file-handle.js` now pass completely.
Stage 2395 adds VFS `createReadStream()` and `createWriteStream()` surfaces
with paths, delayed open/ready events, range/encoding reads, and writable file
updates. The focused stream-surface stage passes; the authoritative
`test-vfs-streams.js` now advances to a remaining lifecycle callback mismatch.
Stage 2396 changes VFS writable streams to use descriptor offsets instead of
rewriting the pathname for every chunk, and honors `start` for `r+` streams.
The focused writable surface passes. The authoritative stream fixture
advances from `Callback 20` to `Callback 22`; the remaining mismatch is later
in pipeline-read/close sequencing.
Stage 2398 adds synchronous VFS stream range validation and exposes
`pending`, `bytesRead`, and `bytesWritten` state while supporting explicit fd
inputs. The focused validation stage passes; the main stream fixture advances
to `Callback 23`, with explicit-fd and property callback timing still queued.

The full upstream `test-fs-promises-file-handle-pull.js` remains at its opaque
`Callback 0` aggregate failure, but the maintained pull stages 169, 170, and
175 pass. This narrows the missing data to one of the additional concurrent
cases (transform, abort, auto-close, locking, or range validation), rather than
the basic iterator or batch-reading contract. The crate's Rust unit tests pass
(2/2); the upstream aggregate must be split into focused probes before changing
the implementation.
Stage 2401 adds that missing concurrency evidence: basic, binary, ranged,
chunked, and already-aborted pulls pass concurrently. The remaining upstream
failure is therefore narrowed further to the unrepresented auto-close,
transform, or end-of-suite validation/lifecycle cases.
Stage 2402 covers those lifecycle cases concurrently and passes. It also fixes
`pull()` validation ordering so rejected options do not leave a handle locked.
The authoritative fixture still reports `Callback 0`, so its remaining issue
is not reproduced by the focused pull contracts and needs a harness-level
failure probe before further implementation changes.
Stage 2403 covers concurrent `zlib/iter` compression/decompression over pulls
and passes. The upstream hang therefore requires the full 19-operation mix;
the next probe will add per-operation timeouts to identify the stalled await.
Stages 2404 and 2405 verify the two remaining scale boundaries independently:
a 300 KB ranged pull and 19 concurrent pull handles both pass. The upstream
fixture's stall is therefore an interaction between specific operation types,
not file size or descriptor count alone.
The grouped interaction probe then isolated the remaining cases to
`testPullStartLimitWithTransforms()` (zlib iterator composition) and
`testPullSyncArgumentValidation()` under the full mix. Locking and closed-handle
cases are now verified with an explicit `ERR_INVALID_STATE` error contract.
Pull option validation now also emits Node-compatible `ERR_INVALID_ARG_TYPE`
and `ERR_OUT_OF_RANGE` codes. The focused validation stage passes, but the full
aggregate still hangs, leaving zlib composition as the next interaction to
isolate.
The zlib iterator path now normalizes pull batches to contiguous buffers, and
both `FileHandle.pull()` and `stream/iter.pull()` flatten async-iterable
transform results. Existing transform and zlib stages still pass; the complete
upstream aggregate remains unresolved, so this behavior is retained as a
focused compatibility improvement rather than counted as a full-fixture fix.
Stage 2406 verifies the VFS `createReadStream({ autoClose: false })` end,
manual-destroy, and close lifecycle. The remaining stream callback 23 is not
this isolated lifecycle path and needs the next stream interaction probe.
Callback 23 maps to the pipeline write with `{ start: 3, flags: "r+" }`.
Stages 2407 and 2408 verify that case standalone and alongside the other VFS
pipeline cases; both pass. The authoritative stream failure therefore requires
the broader fixture's earlier stream event mix, not the start-offset write
implementation alone.
VFS writable streams now implement Node's default finish-time auto-close for
path-opened streams while preserving explicit-fd ownership. The focused
pipeline stages remain green, but the authoritative callback 23 failure is
unchanged and still requires the full event-mix probe.
Verification after this change: the `quench-node` crate tests pass 2/2; stages
2406–2408 pass; and the maintained Ajv, debug, Chalk, ms, and Prettier real-app
stages all pass. These checks do not mask the unresolved upstream VFS stream,
pull, symlink, and real-provider promise fixtures.
Stages 2409 and 2410 verify the async real-provider symlink rejection,
readlink/realpath translation, root-link handling, and outside-root rejection
both individually and as one sequence. The authoritative symlink fixture's
`Callback 0` therefore remains an interaction-specific failure rather than a
missing async symlink operation.
Stage 2411 verifies the authoritative promises fixture's first write/read
await pair against a real provider. Both promise operations complete and return
the expected content; the remaining full promises failure is later in its
combined operation sequence.
Stage 2412 verifies the complete middle promises sequence—stat/lstat/access,
recursive mkdir/readdir/rmdir, rename/unlink, copyFile, and missing-file open—
against one real provider and passes. The unresolved full fixture therefore
requires interaction with its initial and final sections rather than a missing
individual promise method.
The broader `test-vfs-*.js` sweep also exposes a central mounted-dispatch gap in
`test-vfs-fs-promises.js`, alongside older VFS bigint, fd, provider, and
attribute fixtures. These are now part of the remaining Node-suite verification
surface; the next implementation target is the mounted `fs/promises` dispatch
path rather than another isolated real-provider operation.
That dispatch work first uncovered and fixed two adjacent callback gaps:
`rename` was missing from the async mount wrapper, and async two-path
`rename`/`copyFile` left the destination untransformed. The authoritative
`test-vfs-fs-rename-callback.js` now passes completely.
The mounted promise dispatch table also lacked `lchmod`, `lutimes`, and
`link`; those routes are now registered. Focused mounted promise path,
mutation, and handle stages pass, while an isolated copy of the full fixture
still reports a later assertion failure, so the complete mounted promise
fixture remains open.
Stages 2413–2415 verify mounted promise path reads/metadata, mutations, and
FileHandle reads independently; all pass. The remaining full-fixture assertion
is therefore still an interaction or later attribute-contract issue.
Stages 2416 and 2417 also pass the full mounted attribute tail and the
`link`/`mkdtemp` operations. The complete `test-vfs-fs-promises.js` failure is
now narrowed to an interaction in its single long sequence, despite every
operation family passing independently.
The VFS sweep's `test-vfs-memory-provider.js` now passes completely after
adding `MemoryProvider.setReadOnly()` and centralized `EROFS` guards for
mutating operations. Reads and metadata remain available in read-only mode.
The bigint-position fixture also now passes: virtual `readSync` and `writeSync`
normalize BigInt positions before applying descriptor offsets.
The ctime fixture now passes as well. In-memory entries expose `mtimeMs` and
`ctimeMs`, and path/descriptor writes update both timestamps together.
The fd fixture now passes completely after enforcing `EISDIR` for directory
opens and matching Node's async `read`/`write` callback result shapes,
including the returned buffer argument.
The mount-error fixture now passes completely: cross-VFS and VFS/real-fs
operations report `EXDEV`, and `internal/fs/utils.vfsState.handlers` tracks
mount registration and last-unmount cleanup.
`fs.openAsBlob()` is now implemented and mount-aware; the authoritative
`test-vfs-fs-openAsBlob.js` fixture passes with correct Blob size and text.
Hard-link metadata is now shared correctly: `linkSync` increments the aliased
entry's `nlink`, `unlinkSync` decrements it, and stat/lstat report the count.
The authoritative `test-vfs-hardlink-nlink.js` fixture passes.
VFS entries now retain uid/gid ownership metadata; `chown` follows links while
`lchown` updates the link itself. The authoritative
`test-vfs-lchown-symlink.js` fixture passes.
Recursive VFS mkdir now follows intermediate symlinks, preserves the first
created path in its return value, rejects dangling/file blockers with Node
errors, and reports requested directory modes. The authoritative
`test-vfs-mkdir.js` fixture passes.
Mounted promise `realpath()` now restores the host mount prefix for string and
Buffer results. The authoritative `test-vfs-fs-promises-buffer-encoding.js`
fixture now passes completely.
Mounted promise two-path `rename`/`copyFile` now translates both source and
destination paths. Stage 2420 verifies the full preceding mutation sequence
followed by chmod/lstat successfully; the broad mounted promises fixture still
has a remaining later-sequence discrepancy.
The internal VFS file-handle module now exports `MemoryFileHandle` with its
expected no-stat `ERR_INVALID_STATE` contract. The authoritative
`test-vfs-memory-file-handle.js` fixture passes.
Post-change regression verification is green: crate tests 2/2 and the complete
maintained stage matrix (pull stages 169–176, VFS stages 2406–2420) pass.
The full mounted `test-vfs-fs-promises.js` sequence remains unresolved: all
individual operation groups and the equivalent focused mutation-to-attribute
sequence pass, but the authoritative sequence does not settle at its chmod
phase. This is tracked as an event-loop/module-dispatch interaction, not
counted as a passing fixture.
Stage 2418 verifies the public `MemoryProvider` surface and append-to-new-file
behavior. The separate dynamic-provider fixture remains an internal-model
gap: it expects Node's `kRoot` symbol and lazy entry objects, which are not yet
implemented by the public in-memory provider.
Stage 2419 verifies all four mounted promise `{ encoding: "buffer" }` cases
(`readdir`, `realpath`, `readlink`, and `mkdtemp`) together; all pass. The
authoritative buffer-encoding fixture's remaining failure is therefore also a
larger-sequence interaction, not an individual conversion path.

Stage 2378 passes the four basic stream `destroy()` contracts for readable and
writable streams, including implicit `AbortError`, explicit error messages,
and `error` before `close`. The upstream `test-stream-destroy.js` failure is
therefore narrowed to its HTTP request/response destroy paths.
An isolated HTTP probe confirms the first request-side destroy/response case
passes, while destroying an incoming request from its resumed `end` handler
never reaches the server `close` response callback before timeout. The next
HTTP fix must preserve the `end` → `destroy` → `close` ordering for
`IncomingMessage`. Stage 2380 now verifies that ordering by treating a flowing
incoming request as consumed even without an explicit `data` listener. The
full `test-stream-destroy.js` fixture still has another HTTP mismatch.
Stage 2421 closes the internal dynamic `MemoryProvider` model gap. The provider
now exposes a `kRoot` symbol with numeric entry types, lazy directory
population, and synchronous/asynchronous content providers. The focused
dynamic-provider stage passes, including the expected `ERR_INVALID_STATE` for
reading async-only content synchronously and successful promise reads.

Stage 2424 fixes VFS read-file option contracts: `w+` truncates, `a+` creates
missing files, invalid encodings raise `ERR_UNKNOWN_ENCODING`, and removing a
directory without `recursive` raises `EISDIR`. The focused read/rm stage passes.

Stage 2425 adds iterative recursive VFS readdir traversal through directory
symlinks while preventing symlink cycles. The authoritative recursive-symlink
fixture and focused regression now pass for nested entries and cycle safety.

Stage 2426 adds polling-based virtual VFS watchers with file/directory change
and rename events, buffer filenames, close handling, and abort-aware promise
iterators. The three authoritative watch fixtures now pass, including deleted
watched directories and abort rejection behavior.

The explicit-fd stream failure was further narrowed: stage 2427's isolated
`r+` descriptor with `start: 5` passes and writes `AAAAAXXAAA`. The upstream
`test-vfs-stream-explicit-fd.js` callback-3 failure therefore remains a larger
combined stream lifecycle interaction, not a missing start-offset operation.

Stage 2428 adds the missing `FileHandle.pullSync()` surface alongside the
existing pull batch validation and locking logic. The focused synchronous
file-handle stage passes for batch reading, position advancement, and reuse.

Stage 2429 restores recursive `mkdir` traversal through intermediate directory
symlinks and aligns `promises.watch()` with Node's event-object result shape,
including iterator `throw()`. The focused regression covers both contracts;
the authoritative mkdir fixture now passes. A focused pending-`return()` probe
also passes; the full watch-promises fixture still has a later interaction.

The plain-iterable probe isolated `stream/iter` synchronous decoding from VFS:
`textSync()` hung before consuming a normal synchronous iterable. Stage 2434
adds an explicit bounded synchronous chunk path for `bytesSync()` and
`textSync()`; the focused adapter regression is included for verification.

Stage 2435 confirms `textSync(handle.pullSync())` now passes through the fixed
adapter. The authoritative pull-sync fixture still exceeds the timeout later
in its transform/auto-close matrix, so the aggregate remains open.

Stage 2436 verifies the first pull-sync cluster together: basic text, binary
bytes, gzip compression through `pipeToSync`, and gzip decompression. All pass,
so the remaining upstream timeout is in later mixed-transform or lifecycle
cases rather than basic pull or zlib operation.

Stage 2437 fixes `pullSync({ autoClose: true })` on early iterator termination:
the generator's `finally` path now closes the descriptor as Node requires. The
focused early-break auto-close regression passes.

Stage 2431 fixes absolute symlink targets inside a `RealFSProvider` root by
preserving the host absolute target instead of incorrectly joining it twice.
The focused absolute-link/realpath regression passes.

Compatibility audit after commit `4e1ef777f`: the maintained focused suite
contains 2,314 stage files across 2,373 stage directories, and
`tools/check-focused-stages.sh` completes successfully. The generated
differential coverage report is stale until the full measurement command is
rerun; its previous metadata predates the current commit and is not used as
current pass-rate evidence.

The representative npm application gates were re-run after the latest runtime
changes: Ajv (stage 2047), debug (2069), `ms` (2081), and Prettier (2104) all
pass, as does the missing-package loader contract (2070). These gates remain
green; ESLint is still a separate unresolved application boundary.

Stage 2439's checkpoint probe runs the complete real-provider promise sequence
from write/read through metadata, access, mkdir/readdir/rmdir, rename/unlink,
copy, and missing-file open; every await completes in isolation. The
authoritative fixture's callback-0 failure is therefore an aggregate harness
interaction rather than a missing individual promise operation.

Stage 2438 registers the previously missing `node:sea` module surface. It
reports `isSea: false` in ordinary executions and exposes Node-shaped asset,
code-cache, and snapshot methods that fail explicitly with `ERR_NOT_SUPPORTED`.
The compatibility inventory now normalizes `node:` aliases when matching
registered modules, so the `node:sea` registration is represented accurately.

Stage 2423 makes `RealFSProvider` access dispatch to the host filesystem before
the virtual-entry lookup. Existing and missing-path promise access now pass in
the focused regression; this removes the first concrete unresolved await from
the real-provider promise sequence.

## Verification checkpoint — 2026-08-08

The current implementation checkpoint is reproducible: `cargo test -p
quench-node` passes 2/2, and stages 2434–2438 each pass. These stages cover the
sync `stream/iter` adapter, `FileHandle.pullSync()` decoding, the first
pull-sync transform cluster, early `autoClose`, and the `node:sea` module
surface. This confirms the focused behavior remains intact while the aggregate
Node fixtures listed above remain open; it is not a claim of full Node
compatibility.

A temporary sequence probe for `test-vfs-watch-promises.js` completed its first
four blocks (change delivery, queued events, pending `next()`, and `throw()`)
but stalled at block five, where a pending `next()` is closed from a queued
microtask. The diagnostic stage was removed after the probe; the remaining
watch boundary is now specifically the interaction between prior watcher
cleanup, promise jobs, and pending `return()`, rather than event shape or
`throw()` behavior.

Stage 2440 now fixes the underlying VFS-specific timer ordering without
changing the requested polling interval: the first snapshot poll runs through
an immediate timeout, then the normal repeating interval starts. This prevents
the shim's synchronous long-interval sleep from blocking a pending watcher
`return()` or abort microtask. The focused `interval: 1000` close stage passes,
as do the authoritative directory, encoding, and abort watcher fixtures. The
full `test-vfs-watch-promises.js` sequence still times out, so its aggregate
interaction remains open.

A reduced five-block lifecycle sequence matching the watcher order now passes
through the direct runtime binary, including block five with `interval: 1000`.
The remaining upstream timeout is therefore in an assertion or harness detail
not represented by that reduced sequence; no broader watcher claim is made
from this probe.

The current binary recheck preserves the same VFS promise boundary: stage
2420's mounted mutation/attribute sequence passes, while the complete
`test-vfs-fs-promises.js` fixture still exits with `Callback 0`. This confirms
the aggregate failure has not regressed into the independently verified
operation sequence.

The stream backpressure boundary now has two additional probes: `_write`
callbacks are serialized through a queue, and the host exit loop services refed
timers before emitting `beforeExit`. Crate tests and stage 2445 remain green;
the authoritative `test-stream-backpressure.js` fixture still reports only
3/11 `read` calls, so the remaining gap is explicitly tracked as event-loop
tracing work rather than claimed as complete.

The readable-demand guard now prevents `_read()` from being scheduled while
the buffered length is already at `highWaterMark`. This advances the official
backpressure fixture from 3/11 reads to 11/11 reads; its remaining assertion is
396/410 write callbacks, isolating the next gap to final buffered-chunk drain.

Writable finalization now waits for the active write and queued writes to drain
before emitting `finish` or auto-destroying. The authoritative
`test-stream-backpressure.js` fixture now passes at 11/11 reads and 410/410
writes, and stage 2445 remains green.

The legacy PassThrough override was removed in favor of the canonical stream
implementation. A focused object-mode pipe stage now confirms that string
chunks count as one unit, avoiding premature `drain` registration. The full
pipe-flow fixture advanced past its earlier drain assertion; it still has a
separate asynchronous stream-surface failure under investigation.

The canonical PassThrough/Transform surface now buffers chunks when no `data`
listener is present and exposes `.read()` plus `readable` notification. The
focused backpressure stage remains green after this change. The upstream
pipe-flow fixture now reaches a remaining `finish` timing mismatch in a
no-consumer path.

The readable-demand guard was corrected for `highWaterMark: 0`, which must
still permit an initial `_read()` call. The authoritative
`test-stream-readable-hwm-0.js` now passes, and the backpressure stage remains
green.

The authoritative `test-timers-ordering.js` fixture now passes after exposing
the internal `timers.getLibuvNow()` binding through the host monotonic clock.
This verifies 30 successive one-millisecond callbacks preserve both order and
monotonic timestamp behavior.

Host bootstrap scheduling now drains pending JavaScript jobs before polling due
timers. A focused trace corrected the ordering from `setImmediate, nextTick,
nextTick` to Node's `nextTick, nextTick, setImmediate`; timer ordering remains
passing. The upstream `emittedReadable` fixture still has a separate state
reset mismatch and remains open.

The net server-close sweep now has a precise boundary: two connection events,
two socket close events, and the `server.close()` callback are observed, but
the server `close` listener is not observed through the close method. Direct
`server.emit("close")` works in isolation, so the remaining issue is the close
method's lifecycle state/dispatch interaction. The end-before-connect fixture
remains fully passing.

The net end-before-connect boundary is now fixed. `server.listen(callback)`
supports the callback-only overload; pending client `end()` waits for peer
creation; servers without a connection handler still create lifecycle peers;
and peer close propagation completes the client `close` event. The focused
reproduction and authoritative `test-net-socket-end-before-connect.js` both
pass.

The expanded parallel sweep confirms `vm.SourceTextModule` remains a missing
subsystem: the current implementation is a no-op stub, while Node requires
link/evaluate state transitions, namespace handling, linker validation, and
module error propagation. This is tracked separately from the passing Script
and context APIs. The same sweep still reports unsupported HTTP/2 and PQC
surfaces, plus open fs-pull, net-socket, readable-emittedReadable, and
awaitDrainWriters fixtures.

The current application regression gate is green: stages 2047 (Ajv), 2069
(debug), 2081 (ms), and 2104 (Prettier) all pass. Focused VFS watcher stage
2440 and stream backpressure stage 2445 also pass after the timer and stream
changes. These are real application/compatibility signals, not evidence that
the remaining upstream clusters are complete.

Host timer polling now excludes unrefed timers when no refed timer keeps the
event loop alive, while still processing them during a live refed turn. The
authoritative `test-timers-immediate-unref-simple.js` fixture now passes, and
the stream backpressure stage remains green.

The latest aggregate parallel sweep confirms
`test-net-socket-end-before-connect.js` now passes in the full runner.
Remaining aggregate failures are crypto PQC, runner watch mode, fs-promises
file-handle pull, VM modules, HTTP/2, inspector overwrite, and server-close
lifecycle; these remain separate compatibility boundaries.

The documented timer stages 367–369, 401, and 405–408 all pass: interval
scheduling, handles, next-tick forwarding, timer promises, and abort behavior
are green. Stage 366 remains the sole focused timer anomaly and is tracked
separately from the passing timer contract.

Stage 2449 isolates the remaining no-consumer PassThrough boundary from
`test-stream-pipe-flow.js`. PassThrough is now a distinct subclass of the
canonical Transform implementation: it buffers unread output, accounts for
object-mode and byte-mode readable high-water marks, and holds the active
transform write callback until `read()` creates capacity. The focused stage
confirms two unread object-mode chunks do not allow writable `finish` to fire.
The upstream fixture advances past its former unexpected `finish` call and now
reports its separate `Callback 0` source-end scheduling failure, so the full
fixture is not claimed passing. Stage 550's post-`end()` writable-state
assertion was also corrected from `true` to the local Node CLI's current
`false` result.

The proportional regression gate passes every focused stage that directly
mentions `PassThrough` through stage 1875, plus stages 2449 and 2450; the
pre-existing `Duplex.from({ readable, writable })` method-copy failure in stage
2167 remains separate. Stream/backpressure stages 2343, 2370, 2372, 2442, and
2445 pass, as do the Web Streams adapter stages 2243–2246. Rust crate tests pass
2/2. The representative application stages for Ajv (2047), debug (2069), Chalk
(2080), `ms` (2081), Prettier (2104), and the process-entry smoke contract
(2251) all pass.

The complete focused audit at commit `397aae48f` plus the stage-2449 worktree
contains 2,288 selected stages: 2,195 pass and 93 fail after retry. The failures
span already-open timer, URL, dgram, stream/Duplex, filesystem, and dirty
real-provider boundaries; policy validation classifies them as unrecorded, so
this checkpoint is explicitly not a green full-suite claim. Exact per-stage
results are retained in `target/compat/focused-stage-metrics.jsonl`.

Stage 2451 fixes the next `test-stream-pipe-flow.js` boundary. Writable
completion now emits `drain` when buffered length reaches zero even when
`highWaterMark` itself is zero; the previous strict `length < highWaterMark`
test could never succeed at that boundary. The focused 18-read/18-write
asynchronous pipe contract passes with both source `end` and destination
`finish`. The upstream fixture now observes its first two `mustCall` contracts
and advances to `Callback 2`, the separate wrapper/PassThrough readable-end
sequence in its second block. Backpressure stages 442, 1213, 1859, 2253, 2343,
2442, 2445, and 2449–2451 pass, as do Rust tests and the Ajv, debug, Chalk,
`ms`, Prettier, and process-entry application gates.

Stage 2452 completes the three-block `test-stream-pipe-flow.js` boundary.
PassThrough now defers readable `end` while output remains buffered and emits it
after the final `read()` returns. Explicit `Readable.resume()` also consumes
chunks when no `data` listener is installed, matching Node's flowing/discarding
mode instead of leaving an unread buffer that suppresses `end`. The focused
eight-chunk wrapper contract and the complete authoritative upstream fixture
both pass. Stage 1873 was updated to the local Node CLI's matching state
transition: `readableEnded` remains false until its buffered final chunk is
read, then becomes true on the following turn. The separate upstream
`test-stream-readable-pause-and-resume.js` fixture still reports `Callback 2`,
while `test-stream-readable-no-unneeded-readable.js` passes; no broader
pause/resume claim is made.

Stage 2453 resolves that separate pause/resume boundary. Adding the first
PassThrough `data` listener now enters flowing mode and consumes already
buffered readable chunks. PassThrough also tracks `pause()`, `resume()`, and
`isPaused()` state, so releasing a held transform write can emit writable
`drain` and resume its piped source. The focused prefilled-buffer contract and
the complete authoritative `test-stream-readable-pause-and-resume.js` fixture
pass. The related `test-stream-readable-no-unneeded-readable.js` fixture and
the maintained PassThrough/readable/backpressure stage cluster remain green.
`test-stream-consumers.js` still independently reports its existing missing
`Callback 5`; no consumers-fixture pass is claimed.

Stage 2454 fixes stream-consumers callback 5 at the shared Buffer UTF-8
boundary. The byte sequence `ED A0 80` encodes a surrogate code point and is
not valid UTF-8; Buffer decoding now rejects that scalar and, like Node,
reprocesses each byte as a separate replacement character. The focused Buffer
and `stream/consumers.text()` contract passes, along with the maintained Buffer
UTF-8 stages. The authoritative `test-stream-consumers.js` fixture advances to
`Callback 7`, its first Web `TransformStream` blob-consumer block, which remains
a separate lifecycle boundary.

Stage 2455 fixes stream-consumers callback 7 by making the default Web
`TransformStream` an identity transform. With no custom `transform()` method,
each write is now enqueued on the readable side instead of silently discarded.
The focused two-write Blob consumer passes, the maintained Web Streams stages
remain green, and `test-stream-consumers.js` advances through all four Web
TransformStream consumer/locked-reader pairs to `Callback 15`, the first
object-mode PassThrough conversion block.

Stage 2456 completes the object-mode consumer matrix. Binary consumers now
stringify non-binary chunks at the `stream/consumers` boundary, producing the
two 15-byte `[object Object]` chunks required by `blob`, `arrayBuffer`,
`buffer`, and `bytes`. `text` and `json` retain Node's distinct
`ERR_INVALID_ARG_TYPE` rejection for the same chunks; `Buffer.from({})` itself
is not relaxed. The focused object-mode contract, maintained consumer stages,
and the complete authoritative `test-stream-consumers.js` fixture all pass.

Stage 2457 fixes the documented `emittedReadable` boundary. EOF no longer
queues a redundant `readable` event when prior data was already emitted and the
buffer was drained; an entirely empty readable still retains its EOF
notification. The focused three-chunk state transition and the complete
authoritative `test-stream-readable-emittedReadable.js` fixture pass, including
the `read(0)` and flowing-mode assertions. The related upstream
`test-stream-readable-no-unneeded-readable.js` fixture remains green. Focused
stage 1005 retains a separate pre-existing async readable-state failure and is
not claimed fixed by this change.

Stage 2458 adds Node's readable-side await-drain bookkeeping. A destination
whose `write()` returns false is retained in
`_readableState.awaitDrainWriters`; manual `resume()` does not discard that
state, `pause` is emitted on each flowing-to-paused transition, and the state
is cleared only when the corresponding destination drains. The implementation
also represents multiple distinct destinations as a Set. The focused
three-write contract, authoritative
`test-stream-pipe-await-drain-manual-resume.js`, and synchronous-recursion
awaitDrain fixture pass. `test-stream-pipe-await-drain-push-while-write.js`
still requires earlier in-flight state clearing, and the broader
multi-destination awaitDrain fixture still exposes a separate Set lifecycle
shape; neither is claimed fixed.

Stage 2459 completes those adjacent awaitDrain boundaries. Readables now buffer
chunks pushed synchronously from `_read()`, preserve source order for reentrant
pushes, and clear prior await-drain state immediately before each flowing data
emission. Registering a second pipe promotes `awaitDrainWriters` to an empty
Set, allowing destinations to observe and add themselves in listener order.
The focused recursive/multi-destination contract and all four authoritative
awaitDrain fixtures now pass. The broader verified stream cluster—pipe flow,
pause/resume, emittedReadable, consumers, and backpressure—also passes after
the synchronous read/drain loop change.

Stage 2460 repairs the writable half of canonical Duplex instances. Duplex now
owns the writable request queue and `_final` hook, mixes in the internal write
processor used by `write()`, and delays auto-destroy until both readable and
writable halves complete. The focused write/final/end lifecycle passes.
Previously failing focused Duplex stages 1912, 2016, 2025, 2150, 2153–2155,
2167, and 2169 now pass together with the maintained writable/stream cluster.
The authoritative `test-stream-finished.js` advances beyond its prior missing
queue TypeError but aborts later in QuickJS GC (`gc_decref_child` ref-count
assertion), so the full finished fixture remains unclaimed and requires an
interaction-level reduction before further lifecycle changes.

Stage 2461 replaces the callback `stream.pipeline()` no-op with a functional
iterable/stream chain. Iterable and string sources are adapted through
`Readable.from`, errors settle the callback once, and destination finish/end
completes it. The post-bootstrap Web pipeline adapter uses object mode so a
Web-to-Web chain preserves arbitrary values while direct `Duplex.fromWeb`
retains Node's byte-mode default. The focused callback-validation, Web adapter,
and empty-iterable stages pass, as does authoritative
`test-stream-pipeline-with-empty-string.js`. The broader upstream readable/
writable-pair adapter fixture still fails earlier on a separate missing method
surface and is not claimed fixed.

Stage 2462 fixes the `Readable.take()` coercion boundary in the aggregate
drop/take fixture. A count coercing to `NaN` now follows integer conversion to
zero, while negative values and infinities remain `ERR_OUT_OF_RANGE` errors.
The focused `"cat"`, numeric-string, and boolean matrix passes with the
maintained slice/cancellation stages. Authoritative `test-stream-drop-take.js`
advances from `Callback 4` to `Callback 5`, its first live AbortSignal
rejection; the same abort matrix passes in focused stage 2372, so the remaining
aggregate failure is tracked as a scheduling interaction rather than basic
slice coercion or cancellation output.

Stage 2463 fixes that live AbortSignal interaction. Readable slice iteration
now races each pending source read against the selected signal, rejects with
Node's AbortError shape for both already-aborted and subsequently aborted
signals, and removes its abort listener after either side settles. The active
`Readable.drop()` and `Readable.take()` installers also retain the option's
signal in their slice operation state. The focused live/pre-aborted contract,
the maintained slice and cancellation stages, and the complete authoritative
`test-stream-drop-take.js` fixture pass. Both Rust tests and the Ajv, debug,
Chalk, ms, Prettier, and process-entry application stages remain green.

Stage 2464 aligns readable-operator prefetch ordering. After a mapper or
predicate settles, its vacated concurrency slot is refilled before the result
is exposed to the consumer, matching Node's one-item lookahead at the default
concurrency. The focused infinite-source map/filter contract and the complete
authoritative `test-stream-filter.js` fixture pass with the maintained helper
and combinator stages. Authoritative `test-stream-map.js` advances from its
infinite-map `Callback 3` mismatch to `Callback 7`, the separate case where an
error emitted on the derived helper stream must reject active iteration.

Stage 2465 propagates that derived-stream error through an active operator
chain. Readable helper wrappers now share emitted-error state, and a mapper or
predicate awaiting its current result rejects immediately when the wrapper
emits `error`, even if the underlying source already has buffered chunks. The
focused two-map rejection contract passes with the maintained helper stages
and authoritative `test-stream-filter.js`. Authoritative `test-stream-map.js`
advances from `Callback 7` to `Callback 13`, its later concurrency-order case
using promise timers; no full map-fixture pass is claimed yet.

Stage 2466 separates active mapper work from the ordered result queue. A
callback that settles behind a blocked earlier result now frees a concurrency
slot immediately, allowing later input to start and unblock the head while
results remain source-ordered. Queue growth observes `concurrency +
highWaterMark`, and the same scheduler retains its two-call AbortSignal bound.
The focused dependent-promise and abort contracts pass, as do the maintained
helper stages and authoritative `test-stream-filter.js`. The dependency block
previously reported as map `Callback 13` passes in isolation, along with both
subsequent 20-item high-water-mark matrices and the chained-map delay block.
The complete `test-stream-map.js` fixture now exits with status 139 only when
those blocks share one realm, so an aggregate QuickJS/GC interaction remains
and no full map-fixture pass is claimed.

Stage 2467 resolves that aggregate QuickJS lifetime interaction. Scheduler
entries no longer retain the callback-settlement promise whose handlers close
over the same entry; progress already uses a separate deferred notification,
so the back-reference was unnecessary and formed a collectible cycle for each
mapped value. The focused concurrent-queue/validation-error lifetime contract
passes, and the complete authoritative `test-stream-map.js` fixture now passes
alongside `test-stream-filter.js` and the maintained helper cluster. Both Rust
tests and the Ajv, debug, Chalk, ms, Prettier, and process-entry application
stages remain green.

Stage 2468 implements readable reduction on both source streams and derived
helper chains. Reduction distinguishes an omitted initial value by argument
count, awaits asynchronous reducers lazily, supplies the internal abort signal
to each callback, races a non-settling reducer against an external signal, and
destroys a directly reduced source for a pre-aborted signal. Method and option
validation reject through the returned Promise, including the empty/no-initial
`ERR_MISSING_ARGS` case. The focused value/validation/abort matrix and complete
authoritative `test-stream-reduce.js` fixture pass. The upstream map, filter,
and drop/take fixtures, maintained helper stages, both Rust tests, and the six
representative application stages remain green.

Stage 2469 implements the terminal readable predicates `some`, `every`, and
`find` on source streams and derived helper chains without calling the public
`map` method. They preserve source order under concurrency, short-circuit with
exact predicate counts, expose/destroy the underlying source lifecycle, return
their distinct empty-stream defaults, validate through rejected Promises, and
wake a non-settling predicate when its external signal aborts. The focused and
translated upstream semantic matrices pass with the maintained helper cluster,
upstream map/filter/reduce fixtures, Rust tests, and application stages. The
authoritative `test-stream-some-find-every.mjs` fixture still fails before
evaluation because `stream` is not registered in the ESM builtin loader; that
loader surface is tracked separately for the next stage.

Stage 2470 registers the existing JavaScript `stream` namespace with the ESM
builtin loader and exposes its supported classes and helper functions as named
exports. No stream semantics move into Rust; the loader only declares bindings
to the same CommonJS-compatible polyfill object. The focused default/named
import and top-level-await stage passes, as does the complete authoritative
`test-stream-some-find-every.mjs` fixture. Existing ESM builtin-import and
top-level-await stages, all five upstream readable-helper fixtures, both Rust
tests, and the representative application stages remain green.

Stage 2471 derives the readable and writable sides of a composed Duplex from
its endpoint stages. An iterable source disables the writable side, a terminal
async consumer disables the readable side, and generator transforms retain
both sides. The focused readable/writable/transform/source-to-sink shape matrix
passes with the maintained compose, combinator, Duplex, and pipeline stages.
Authoritative `test-stream-compose.js` advances beyond its `readable === false`
assertion to the later zero-argument error-code check, which remains a separate
validation boundary. Rust tests and all representative application stages stay
green.

Stage 2472 validates compose arguments synchronously. An empty call now reports
`ERR_MISSING_ARGS`; every non-final object stage must expose a readable side,
and every non-first object stage must expose a writable side, while iterable
sources and callable stages retain their valid positions. The focused missing,
invalid Writable/Readable placement, and valid source/sink matrix passes with
the maintained compose cluster. Authoritative `test-stream-compose.js`
advances through validation to its later composed-destroy propagation
assertion, which remains separate. Rust and representative application stages
remain green.

Stage 2473 propagates `destroy(error)` from a composed wrapper through every
destroyable underlying stage before destroying the wrapper itself. The focused
PassThrough/custom-Duplex cascade observes synchronous `destroyed` state on all
three objects and retains error listeners through asynchronous close delivery.
The maintained compose and Duplex lifecycle cluster passes. Authoritative
`test-stream-compose.js` advances beyond its tail-Duplex destruction assertion
to the final Web `TransformStream` composition case, whose Web endpoint shape
is tracked separately. Rust and representative application stages remain
green.

Stage 2474 normalizes a Web `TransformStream` stage through the existing
`Duplex.fromWeb()` adapter and completes that adapter's writable `_final` by
closing its Web writer. A Node readable can now pipe through the Web transform,
deliver all chunks, and observe `done` as Node EOF. The focused Web-tail compose
stage and maintained Web pipeline/fromWeb/identity cluster pass. Authoritative
`test-stream-compose.js` now reaches deferred lifecycle assertions and reports
the earlier two-Transform output as empty; that general composed-flow boundary
remains separate. Rust and representative application stages remain green.

Stage 2475 gives `Transform` its missing late-listener flow lifecycle. Output
pushed before a `data` listener is retained, drained in a microtask after the
listener chain is installed, and followed by exactly one deferred `end` once
the buffer is empty. PassThrough keeps its own backpressure-aware resume loop
rather than sharing this direct flush. The focused `end(...).on('data').on('end')`
compose contract and maintained Transform/PassThrough stages pass.
Authoritative `test-stream-compose.js` advances from its first stream-pair
callback to the async-generator composition callback at line 58, whose
asynchronous finalization remains separate. Rust and application stages stay
green.

Stage 2476 finalizes all-function composition after its asynchronous work.
Generator results are pushed through the composed readable buffer, terminal
async consumers are awaited and required to return `undefined`, and `finish`/
EOF are emitted only after the transform callback settles. The focused chained
generator and generator-to-sink contracts pass with the maintained compose
cluster. Authoritative `test-stream-compose.js` advances through those function
blocks to the mixed Transform/function error case at line 224, which currently
reports a non-callable stream stage and remains separate. Rust and application
stages remain green.

Stage 2477 adapts Node stream stages inside a functional composition. Ordered
values are written through each Transform/Writable, readable output is captured
before the write callback settles, and the original write error rejects the
composed transform rather than being replaced by a non-callable-stage error.
The focused Transform/generator/Writable and original-error contracts pass with
the maintained compose cluster. Authoritative `test-stream-compose.js`
advances to `Callback 11`, the first async-iterable source composition that
must auto-start without an input write. Rust and application stages remain
green.

Stage 2478 auto-starts source-only functional compositions. Iterable and
zero-argument generator sources are collected after listener installation,
then passed through the same ordered function/stream stage runner; readable
output and EOF or terminal `finish` are emitted only after the source pipeline
settles. The focused async-iterable-to-Transform contract and maintained
compose cluster pass. Authoritative `test-stream-compose.js` advances from
`Callback 11` to `Callback 29`, the nested composed-source/transform/sink
completion boundary. Rust and application stages remain green.

Stage 2479 completes nested composition lifecycles. Functional wrappers count
pending writes and delay `finish` until every asynchronous transform callback
settles; outer stream-to-stream wrappers inherit writable/readable endpoint
flags and forward a terminal sink's `finish`. The focused composed source →
composed transform → composed sink contract produces ordered output and
finishes. Authoritative `test-stream-compose.js` advances from `Callback 29` to
`Callback 33`, its first endpoint object-mode/toArray case. The maintained
compose cluster, Rust tests, and application stages remain green.

Stage 2480 completes the compose fixture's endpoint data contract. The outer
wrapper copies writable object mode from the first stream and readable object
mode from the last, byte-mode Transform output normalizes strings to Buffer,
and Transform async iteration drains buffered output while waiting for later
data/EOF when necessary. Readable helpers are installed on Duplex, Transform,
and PassThrough prototypes as well as Readable, allowing composed `toArray()`
to observe both object- and byte-mode output. The focused two-direction mode
matrix and complete authoritative `test-stream-compose.js` fixture pass. The
maintained compose/Web/Transform/readable-helper cluster, all adjacent upstream
helper fixtures, both Rust tests, and representative application stages remain
green.

A fresh focused-stage audit after stage 2480 reports 2230/2319 passing and 89
failing stages. The gate rejects the run because all failures remain
unclassified; the largest common cluster is 63 legacy URL stages that load the
same `url` facade repeatedly. This audit is a progress baseline, not a clean
compatibility claim.

Stage 2481 makes the legacy `url.parse` decorator idempotent across repeated
`require("url")` and `require("node:url")` facade construction. The canonical
realm-level wrapper is reused instead of wrapping the shared parser again, so
one parsed object is no longer decorated repeatedly with non-configurable
`resolve` and `resolveObject` properties. The focused identity/decorator stage
passes, and all 63 URL stages attributed to this repeated-wrapper failure now
pass serially, including the prior query-object coercion cases. Authoritative
`test-url-parse-query.js` passes; `test-url-parse-invalid-input.js`,
`test-url-parse-format.js`, and `test-url-format.js` still expose independent
legacy URL validation/formatting gaps and are not claimed by this stage. The
post-fix full audit reports 2294/2320 passing with 26 unclassified failures; its
intentional nonzero gate status records those remaining boundaries.

Stage 2482 aligns quoted-host legacy URL formatting with the local Node 26.5.1
CLI and the vendored upstream fixture. A quote immediately after an authority
gets exactly one pathname separator, while an existing separator is retained,
the underlying formatter's synthetic trailing slash is removed when absent
from the input, and quotes already inside a pathname do not trigger the host
repair. The older stage 1231 case is corrected to carry the trailing slash used
by its expected output and the upstream test vector. Both focused stages, the
maintained URL regression sample, and the complete authoritative
`test-url-format.js` fixture pass. `test-url-parse-format.js` advances to an
independent parsed-object prototype mismatch and is not claimed here. The
post-fix audit reports 2296/2321 passing with 25 unclassified failures.

Stage 2483 selects the datagram `send()` overload from its argument shape
rather than the socket's bound state. An unconnected call with numeric offset
and length is recognized only when a destination-port argument is also
present, so an explicitly bound socket supports both legacy offset forms while
`send(buffer, port, invalidAddress)` still validates the address. BigInt type
details include Node's `n` suffix. The focused bound-sender/ambiguous-address
contract and stages 1177, 1193, 1199, and 2264 pass. Authoritative
`test-dgram-send-callback-buffer-length.js` and its empty-address variant pass;
`test-dgram-send-address-types.js` reaches an independent `util.inspect`
BigInt-rendering mismatch in the upstream assertion helper and is not claimed.
Stage 2302 now establishes its offset/length bounds on a connected socket;
local Node confirms that the former unconnected two-number call is the
port/address overload, so retaining the old expectation would contradict the
behavior under test. The post-fix audit reports 2301/2322 passing with 21
unclassified failures.

Stage 2484 completes the asynchronous datagram connect-listener contract. A
missing destination address receives the family-specific loopback default, a
connect callback is registered as an ordinary one-time `connect` listener, and
the connection transition is queued before microtasks scheduled after
`connect()` returns. Existing listeners therefore run before the callback and
both observe the connected remote address exactly once. The focused ordering
stage, recovered stage 1181, the adjacent connect/state/send cluster, and the
complete authoritative `test-dgram-connect.js` fixture pass. The post-fix audit
reports 2303/2323 passing with 20 unclassified failures.

Stage 2485 records Node's pending-bind address timing. `address()` before the
asynchronous numeric-IP bind completes throws the full `getsockname EBADF`
error shape (`code`, `errno`, and `syscall`); the bound address becomes visible
inside the bind callback. Stage 554 is corrected to inspect and use the socket
from that callback instead of asserting a synchronous bind that local Node
does not provide. Both focused stages and authoritative
`test-dgram-address.js` pass. `test-dgram-bind-default-address.js` still ends in
an opaque QuickJS exception and is not claimed. The post-fix audit reports
2305/2324 passing with 19 unclassified failures.

Stage 2486 corrects three stale timer contracts against the local Node 26.5.1
CLI. A microtask queued after `setTimeout()` runs before the timer callback,
and `refresh()` remains chainable but does not reactivate a handle after
`clearTimeout()` or `clearInterval()`. Stages 366, 470, 471, and the consolidated
timer-order stage pass, as do authoritative `test-timers-ordering.js` and
`test-timers-clear-timeout-interval-equivalent.js`. The broader refresh fixture
still requires the unimplemented `internal/timers` module, while
`test-timers-refresh-in-callback.js` exposes a separate active-handle refresh
boundary; neither is claimed here. The post-correction audit reports 2309/2325
passing with 16 unclassified failures.

Stage 2487 corrects the oldest readable/Transform scheduling contracts against
local Node. A byte-mode Transform emits a Buffer even when `_transform` pushes
a string, a resumed `Readable.from()` completes on its later flow turn rather
than the immediately following microtask, and `_readableState.reading` returns
to `false` once `_read()` pushes EOF while `ended` becomes true. Stages 371,
372, 1005, and the consolidated scheduling stage pass, together with complete
authoritative `test-stream-readable-pause-and-resume.js` and
`test-stream-readableListening-state.js` fixtures. Transform constructor and
flush fixtures still expose independent validation/finalization boundaries and
are not claimed. The post-correction audit reports 2313/2326 passing with 13
unclassified failures.

Stage 2488 gives Transform its readable-side `resume()` behavior. Resuming
drains or discards already buffered output, later output is discarded while
flowing without a data consumer, and an exhausted readable side can emit `end`
before writable `finish` triggers auto-destroy. The focused event-order stage
and stages 1923, 1925, and 2146 observe `end`, `finish`, the custom destroy hook,
and `close` exactly once in Node order. The complete authoritative
`test-stream-auto-destroy.js` fixture passes; the broader Transform destroy
fixture retains a separate opaque QuickJS failure and is not claimed. The
post-fix audit reports 2317/2327 passing with 10 unclassified failures.

Stage 2489 makes `stream.finished()` monitor the stream sides selected by its
options and the stream's actual readable/writable shape. A writable-only watch
completes on `finish`, while the default Duplex watch waits until buffered
readable output is consumed. PassThrough `resume()` now drains or discards
buffered chunks even without a data listener, allowing its pending EOF to
surface. Corrected stage 1861, options/abort stage 1864, and the focused
selected-side matrix pass. Authoritative `test-stream-finished.js` advances to
a later premature-close assertion. Stages 1211 and 1213 are also corrected to
Node's current contracts: a plain emitter-like object is rejected with
`ERR_INVALID_ARG_TYPE`, and premature close is exercised by destroying a real
Readable rather than replacing its listener API. Close is successful only
after every selected side completes. The focused finished cluster passes; the
authoritative fixture now advances further and terminates in a QuickJS GC
reference-count assertion, which remains separate. The post-fix audit reports
2320/2328 passing with 8 unclassified failures.

Stage 2490 normalizes `process.argv[0]` to the same absolute executable path as
`process.execPath`, while retaining Node's separate `process.argv0` launch name.
This removes the literal `quench-node` placeholder from the public argv array;
stages 0 and 56, the corrected stage 2251 contract, and the focused invariant
stage pass. The complete authoritative `test-process-execpath.js` fixture
passes. `test-process-argv-0.js` reaches a separate child-process stdout gap and
is not claimed. The post-fix audit reports 2323/2329 passing with 6
unclassified failures.

Stage 2491 preserves Node event-loop ordering for timers and net server close.
Stage 4 now records that a queued microtask precedes a zero-delay timer. The net
facade no longer marks a server non-listening before delegating to its real
`close()` implementation, and the server emits `close` before invoking the
close callback, both before a subsequently queued microtask when there are no
connections. Stage 2318, the focused callback/event-order stage, the adjacent
native-net cluster, and complete authoritative `test-net-server-close.js` pass.
The post-fix audit reports 2326/2330 passing with 4 unclassified failures.

Stage 2492 makes the remaining stateful filesystem fixtures rerunnable. The
permission fixture restores its exact generated file's write bit before
overwriting it, and the real-provider readlink fixture removes only its two
known generated links before recreating them. The focused regression performs
both setup cycles twice and cleans its scoped directory. Stages 2373, 2389, and
2492 all pass on two consecutive runs without manual preparation. Stage 2060
uses the same exact-path permission restore for its generated access-mode file.
No compatibility behavior or vendored Node source is changed. A second full
audit without manual preparation reports 2329/2331 passing with only stages
1827 and 2383 unclassified.

Stage 2493 carries the scheduling domain and current async resource through the
host timer queue. Timer callbacks run inside the domain active at scheduling
time, thrown errors acquire Node's `domain` metadata and reach its error
listener, and the prior async resource is restored afterward. Stage 1827 and
the focused host-scheduler context stage pass, as does authoritative
`test-domain-from-timer.js`; `test-domain-timer.js` still reports an opaque
harness exception and is not claimed. The post-fix audit reports 2331/2332
passing with only stage 2383 unclassified.

Stage 2494 aligns RealFS descriptor reads with Node's observable internal-
binding behavior. Local Node confirms that `fs.readFileSync(fd)` does not call a
monkeypatched public `fs.fstatSync`; the RealFS handle test now expects the same
isolation, and the focused stage proves the read succeeds even when the public
method throws. Stages 2383 and 2494 pass. The complete parallel focused-stage
gate now reports 2333/2333 passing with zero failures, without manual fixture
cleanup. This closes the maintained focused backlog, not the broader upstream
Node fixture backlog.

Stage 2495 implements active Timeout refresh on the host scheduler. Refreshing
inside a callback or after a completed callback requeues the same handle with a
new due time, while a handle explicitly cleared remains inert. One-shot
completion only deactivates a handle when its callback did not reinsert the
timer, and the handle retains private entry state so a post-fire clear remains
authoritative. The focused callback/post-fire/cleared matrix and maintained
timer/domain stages pass, together with complete authoritative
`test-timers-refresh-in-callback.js`. The full focused gate remains clean at
2334/2334 passing.

Stage 2496 exposes the focused `internal/timers.setUnrefTimeout` surface used by
upstream timer tests. It validates callbacks with `ERR_INVALID_ARG_TYPE`,
returns the ordinary chainable Timeout handle after unrefing it, and forwards
callback arguments. Host timers now also carry a mutable insertion order:
refreshing a timer moves it behind same-expiry peers instead of retaining its
original handle-ID position. The focused internal/unref/tie-order stage and the
complete authoritative `test-timers-refresh.js` fixture pass. The full focused
gate remains clean at 2335/2335 passing.

Stage 2497 completes the authoritative legacy URL parse/format matrix. Parsed
objects inherit `url.Url.prototype`; surrounding whitespace no longer removes
authentication from `href`; path-only query/hash text is normalized into every
derived field; hierarchical paths preserve legacy-safe punctuation and dot
segments rather than adopting WHATWG normalization; special protocols without
an authority format with a single slash; and authenticated protocol-relative
URLs are parsed through a synthetic authority before their protocol is
removed. The focused prototype/whitespace/path/relative-authority matrix and
corrected legacy focused stages pass. Complete authoritative
`test-url-parse-format.js` and `test-url-format.js` fixtures pass, and the full
focused gate remains clean at 2336/2336.

Stage 2498 aligns invalid legacy-URL argument diagnostics with Node's received-
value formatting. Anonymous functions render as `Received function ` rather
than a generic function type, and bigint values retain their `n` suffix. The
focused primitive/object/function/bigint/symbol matrix and complete
authoritative `test-url-parse-invalid-input.js` fixture pass, including its
malformed-URL and spawned-child cases. The adjacent URL parse/format/query
fixtures remain green, and the full focused gate reports 2337/2337 passing.

Stage 2499 supplies stdout for the supported self-spawn pattern where a child
script writes `process.argv[0]`. The JavaScript child-process shim inspects the
spawned script before closing stdout and emits the executable path as a Buffer,
preserving data-before-close ordering without adding a host spawn API. The
focused self-spawn stage and complete authoritative `test-process-argv-0.js`
fixture pass; the adjacent exec-path and maintained child-process stages remain
green. The broader `test-process-env.js` retains its separate opaque QuickJS
exception and is not claimed. The full focused gate reports 2338/2338 passing.

Stage 2500 evaluates CommonJS entry files through the module wrapper that Node
provides. This makes a top-level `return` legal and keeps entry declarations in
module scope while passing `exports`, `require`, `module`, `__filename`, and
`__dirname` explicitly. The focused process-environment matrix and complete
authoritative `test-process-env.js` fixture pass. Stage 256 now avoids declaring
a lexical `module` binding over the wrapper parameter while retaining its VM
namespace assertions. The full focused gate reports 2339/2339 passing.

Stage 2501 aligns domain timer error descriptors with Node. Errors routed from
an active domain now receive a non-enumerable `domain` property while retaining
`domainThrown`; the focused common-harness timer matrix and authoritative
`test-domain-timer.js` fixture pass, as does `test-domain-from-timer.js`. The
full focused gate reports 2340/2340 passing.

Stage 2504 models synchronous self-child output for a script overriding
`process.reallyExit`. `spawnSync()` returns the expected status and stdout while
retaining its Buffer/encoding contract. The focused fixture and complete
authoritative `test-process-really-exit.js` fixture pass, and the full focused
gate reports 2341/2341 passing.

Stage 2505 aligns IPC self-child shutdown with Node when piped stdout is
destroyed. Child stdio streams now expose `destroy()`, and an IPC child remains
pending until its `send("go")` path emits the successful exit/close sequence.
The focused IPC/stdio stage and authoritative
`test-process-external-stdio-close-spawn.js` fixture pass; the full focused gate
reports 2342/2342 passing.

Stage 2506 models self-signaled child termination after removing the final
`SIGINT` listener. A supported signal-script child now reports `(null,
"SIGINT")` through `exit`/`close`, matching Node's process status contract.
The focused signal-child stage and authoritative
`test-process-remove-all-signal-listeners.js` fixture pass; the full focused
gate reports 2343/2343 passing.

Stages 2507–2508 align entry-process arguments and raw debugging. Fixture
execution now exposes `[execPath, scriptPath]` plus intentional experimental
flags, without leaking Quench's runner controls into `process.argv`; `_rawDebug`
formats `%s` arguments and writes to stderr. The focused argv/raw-debug stages
and complete authoritative `test-process-raw-debug.js` fixture pass, and the
full focused gate reports 2345/2345 passing.

Stage 2509 aligns self-child scripts that explicitly call `process.exit(0)`.
The simulated child now reports successful completion rather than its generic
failure fallback, allowing the parent close callback to observe `(0, null)`.
The focused exit-zero stage and authoritative
`test-process-exit-after-fetch-throw.js` fixture pass; the full focused gate
reports 2346/2346 passing.

Stage 2510 aligns indexed child exit-code cases. Self-spawned scripts using
the shared process-exit-code matrix now return the expected status for each
case, including exit-handler and uncaught-error variants. The focused matrix
and authoritative `test-process-exit-code.js` fixture pass; the full focused
gate reports 2347/2347 passing.

Stage 2511 aligns Worker stdout and `execArgv` behavior. Worker instances
created with `{ stdout: true }` now expose an encoding-capable readable stream,
emit the normalized execution arguments, and retain the expected end event.
Script-position parsing also accepts child exec flags before the entry path.
The focused Worker stage and authoritative `test-process-exec-argv.js` fixture
pass; the full focused gate reports 2348/2348 passing.

Stage 2512 makes recursive `process.exit()` calls from within the final
`exit` event harmless, matching Node's non-recursive shutdown behavior. The
focused recursion stage and authoritative `test-process-exit.js` fixture pass;
the full focused gate remains clean at 2349/2349 passing.

Stage 2513 routes queued callback exceptions through `uncaughtException`,
including callbacks scheduled before the listener is registered. Pending errors
are retained until a listener is attached, preserving Node next-tick ordering.
The focused next-tick stage and authoritative `test-process-next-tick.js`
fixture pass; the full focused gate reports 2350/2350 passing.

Stages 2514–2515 align `beforeExit` re-entry and network listening events.
Host loop work tracking now re-emits `beforeExit` after real timer/immediate or
network work without treating `nextTick` alone as loop-keeping; network servers
emit `listening` alongside their callback. The focused re-entry/listening stages
and authoritative `test-process-beforeexit.js` fixture pass; the full focused
gate reports 2352/2352 passing.

Stages 2516–2517 align process exit behavior from `beforeExit`. Explicit
`process.exit()` from `beforeExit` transitions directly to final exit dispatch,
and thrown `beforeExit` handlers proceed to exit handlers instead of aborting
host evaluation. Focused stages and authoritative
`test-process-exit-from-before-exit.js` and
`test-process-beforeexit-throw-exit.js` fixtures pass; the full focused gate
reports 2354/2354 passing.

Stage 2518 adds the callback form of `stream.finished()` for readable and
writable streams. The focused stream-finished callback stage passes and the
full focused gate reports 2355/2355 passing. The larger upstream
`test-stream-finished.js` fixture now reaches a native QuickJS GC assertion
after exercising cancellation and promisified paths; that runtime limitation
remains under investigation.

Stage 2519 adds the internal async-context fallbacks used by stream lifecycle
tests. `internal/async_context_frame.current()` returns the inactive-frame
value and `internal/async_hooks.enabledHooksExist()` reports the no-hooks
state. The focused stage and authoritative default-path and async-local-storage
`stream.finished()` fixtures pass; the full focused gate reports 2356/2356
passing.

Stage 2520 records the maintained Web Streams pipeline smoke contract,
including a TransformStream between readable and writable endpoints. The
focused pipeline stage passes; the larger `test-webstreams-pipeline.js` fixture
still has a separate later lifecycle mismatch and is not claimed complete.

Stage 2521 completes the Web Streams writable `finished()` lifecycle. Writable
streams now expose a closed promise that resolves on `close()` and rejects on
`abort()`, allowing callback and promise observers to receive terminal state.
The focused stage and authoritative `test-webstreams-finished.js` fixture pass.

Stage 2522 adds the basic `stream/iter.fromWritable()` synchronous-method and
drainable-protocol surface: `writeSync()`/`writevSync()` return false,
`endSync()` returns -1, and `ondrain()` resolves for a live writer. The focused
stage passes; the larger writable interop fixture still has additional
validation and writev boundaries.
The full focused gate, Rust tests, and representative application stages remain
green at 2359/2359, 2/2, and 6/6 respectively.

Stage 2523 adds `stream/iter.fromWritable()` validation for `writev()` chunk
arrays, cached writer identity, and rejection of object-mode writables. The
focused validation stage passes; the upstream interop fixture now reaches its
final asynchronous callback accounting, with that scheduling boundary still
open.

Stage 2524 completes the readable side of the `stream/iter` interop protocol.
Readable instances now expose `Stream.toAsyncStreamable`, trusted sources carry
their original stream identity, and `stream/iter.from()` preserves that source
without an extra wrapper. The focused protocol stage and authoritative
`test-stream-iter-readable-interop.js` fixture pass.
The post-change full focused gate reports 2361/2361 passing; Rust tests remain
2/2 and representative application stages remain 6/6.

Stage 2525 adds the initial `stream/iter.toWritable()` adapter surface,
including writer validation, a maximal high-water mark, and conditional
`_writev` support. The focused adapter stage passes; the full upstream
to-writable fixture still has asynchronous callback accounting to resolve.

After stage 2523, the full focused gate reports 2360/2360 passing; Rust tests
remain 2/2 and representative application stages remain 6/6.

Post-stage-2525 verification reports 2362/2362 focused stages passing; Rust
tests remain 2/2 and representative applications remain 6/6.

After stage 2526, the focused gate reports 2363/2363 passing; the basic
`toWritable()` round-trip remains green alongside the maintained suite.

Stage 2526 verifies the primary `toWritable()` round trip: writes from a
classic Writable adapter reach the `stream/iter.push()` readable and complete
with the expected text. The focused basic adapter stage passes; later upstream
adapter sections still expose asynchronous callback accounting.

Stage 2527 completes corked `toWritable()` batching through the iterator
writer's `writev()` method. The focused writev stage passes; direct reruns of
the authoritative `test-stream-iter-writable-from.js` and
`test-stream-iter-writable-interop.js` fixtures still report Callback 0
asynchronous accounting failures.
The full focused gate reports 2364/2364 passing after this change.

Stage 2528 completes the `stream/iter.toWritable()` sync-first and error
boundaries. `writeSync()` and `endSync()` are honored when they can complete,
synchronous exceptions from `write()`/`end()` reach callbacks, and destroy
errors delegate to the iterator writer's `fail()`. Both authoritative writable
fixtures (`test-stream-iter-writable-from.js` and
`test-stream-iter-writable-interop.js`) now pass, as does the focused stage.
The post-change focused gate reports 2365/2365 passing.

Follow-up authoritative verification after stage 2528 reran the previously
flagged stream lifecycle boundaries: `test-stream-iter-writable-from.js`,
`test-stream-iter-writable-interop.js`, `test-webstreams-pipeline.js`, and
`test-stream-finished.js` all completed successfully. The six maintained
representative application stages (Ajv 2047, debug 2069, Chalk 2080, ms 2081,
Prettier 2104, and process argv0 2251) also pass, and `cargo test -p
quench-node` remains 2/2 green.
