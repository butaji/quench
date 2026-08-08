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
