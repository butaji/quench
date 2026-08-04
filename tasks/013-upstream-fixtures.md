# Upstream fixtures — pass every `tests/node/test/parallel/*.js`

## Goal

The 4684 fixtures in `tests/node/test/parallel/` are the contract surface for
Node. The work is to:

1. Pick a cluster of fixtures by file-name prefix.
2. Write one focused stage (or a small set) that captures the same contract
   as the cluster, in the form the project already uses.
3. Run the original fixtures against the binary via
   `tools/run-node-tests.sh` to confirm parity.
4. Implement the smallest polyfill behaviour that makes both the focused
   stage and the original fixtures pass.
5. Commit one stage per slice.

The project stops being "complete" when
`tools/measure-node-tests.sh tests/node/test/parallel` reports the highest
feasible pass rate (target ≥ 95%; the rest are expected skips for
network/threading/addons/permissions fixtures that the host does not
support).

## Cluster backlog (priority order)

Each row is a slice. The prefix is the file-name prefix in
`tests/node/test/parallel/`. The `~` count is approximate (from
`ls tests/node/test/parallel | grep -c '^<prefix>'`).

| #   | Prefix                 | Count | Module / domain                           | Existing stage(s)                                                                                                                |
| --- | ---------------------- | ----- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `cluster-`             | ~95   | task 009 next slice; cluster / child IPC  | 504, 505, 506, 507, 508, 509, 510, 559, 560, 561, 562, 563, 564, 565, 566, 567, 568 (process IPC)                                |
| 2   | `child-process-`       | ~125  | task 011 / child_process; fork/exec/stdio | 501, 502, 503, 569, 570, 571, 572, 573, 574, 575, 576, 577, 578, 579, 580, 581, 582, 583, 584, 585, 586, 587, 588, 589 (dispose) |
| 3   | `http-`                | ~250  | task 011 / http; server, client, agent    | 494                                                                                                                              |
| 4   | `http2-`               | ~60   | task 011 / http2; session / stream        | — (TODO)                                                                                                                         |
| 5   | `https-`               | ~30   | task 011 / https; TLS over loopback       | — (TODO)                                                                                                                         |
| 6   | `net-`                 | ~80   | task 011 / net; TCP, Server, Socket       | 502 (subset)                                                                                                                     |
| 7   | `dgram-`               | ~90   | task 011 / dgram; UDP                     | — (TODO)                                                                                                                         |
| 8   | `dns-`                 | ~40   | task 011 / dns; lookup, resolver          | — (TODO)                                                                                                                         |
| 9   | `tls-`                 | ~30   | task 011 / tls; TLSSocket, Server         | — (TODO)                                                                                                                         |
| 10  | `fs-`                  | ~400  | task 011 / fs; stream + op + watch        | many; gaps                                                                                                                       |
| 11  | `fs-promises-`         | ~30   | task 011 / fs/promises                    | — (TODO)                                                                                                                         |
| 12  | `buffer-`              | ~110  | task 001; data + encoding                 | many                                                                                                                             |
| 13  | `stream-`              | ~50   | task 004; pipeline / finished             | many                                                                                                                             |
| 14  | `stream-web-`          | ~25   | task 011 / stream/web; WHATWG streams     | — (TODO)                                                                                                                         |
| 15  | `crypto-`              | ~250  | task 005; cipher, sign, key, pqc, hkdf    | 374-421, 484-488                                                                                                                 |
| 16  | `async-hooks-`         | ~50   | task 006; execution resource, init        | 493, 495, 496                                                                                                                    |
| 17  | `async-local-storage-` | ~15   | task 011 / async_hooks                    | — (TODO)                                                                                                                         |
| 18  | `worker-`              | ~70   | task 011 / worker_threads (host)          | — (TODO)                                                                                                                         |
| 19  | `events-`              | ~25   | events; abort listener, custom, on async  | — (TODO)                                                                                                                         |
| 20  | `eventtarget-`         | ~10   | task 012; global EventTarget              | — (TODO)                                                                                                                         |
| 21  | `diagnostics-channel-` | ~50   | task 011 / diagnostics_channel            | — (TODO)                                                                                                                         |
| 22  | `readline-`            | ~30   | task 011 / readline; Interface            | — (TODO)                                                                                                                         |
| 23  | `repl-`                | ~25   | task 011 / repl; minimal                  | — (TODO)                                                                                                                         |
| 24  | `tty-`                 | ~15   | task 011 / tty                            | — (TODO)                                                                                                                         |
| 25  | `assert-`              | ~25   | task 011 / assert; async, deep            | — (TODO)                                                                                                                         |
| 26  | `console-`             | ~25   | task 011 / console                        | — (TODO)                                                                                                                         |
| 27  | `url-`                 | ~25   | url; WHATWG, fileURL                      | — (TODO)                                                                                                                         |
| 28  | `querystring-`         | ~10   | task 007; unicode / unescape              | 489-491                                                                                                                          |
| 29  | `path-`                | ~5    | task 009; posix/win32                     | 179 (corrected)                                                                                                                  |
| 30  | `util-`                | ~25   | util; promisify, parseArgs, styleText     | 181 (format)                                                                                                                     |
| 31  | `timers-`              | ~5    | task 003; order, unref, refresh           | 366-471                                                                                                                          |
| 32  | `perf-hooks-`          | ~5    | task 003; timerify, observer              | 402-412                                                                                                                          |
| 33  | `process-`             | ~10   | task 011 / process; report, signal        | 409-411, 429                                                                                                                     |
| 34  | `vm-`                  | ~25   | task 011 / vm; source module, compile     | — (TODO)                                                                                                                         |
| 35  | `domain-`              | ~30   | task 011 / domain                         | — (TODO)                                                                                                                         |
| 36  | `module-`              | ~10   | task 011 / module; builtin, createReq     | — (TODO)                                                                                                                         |
| 37  | `os-`                  | ~5    | task 011 / os; userInfo, cpus             | — (TODO)                                                                                                                         |
| 38  | `zlib-`                | ~5    | task 011 / zlib                           | — (TODO)                                                                                                                         |
| 39  | `inspector-`           | ~10   | task 011 / inspector (skip on host)       | — (skip)                                                                                                                         |
| 40  | `trace-events-`        | ~3    | task 011 / trace_events (skip)            | — (skip)                                                                                                                         |
| 41  | `wasi-`                | ~3    | task 011 / wasi (skip)                    | — (skip)                                                                                                                         |
| 42  | `punycode-`            | ~3    | task 011 / punycode (alias)               | — (TODO)                                                                                                                         |
| 43  | `v8-`                  | ~5    | task 011 / v8                             | — (TODO)                                                                                                                         |
| 44  | `abortcontroller-`     | ~10   | task 011 / abort (abort controller)       | — (TODO)                                                                                                                         |
| 45  | `abortsignal-`         | ~5    | task 011 / abort (signal)                 | — (TODO)                                                                                                                         |
| 46  | `blob-`                | ~5    | task 011 / blob; stream                   | — (TODO)                                                                                                                         |
| 47  | `broadcastchannel-`    | ~3    | task 012 / BroadcastChannel               | — (TODO)                                                                                                                         |
| 48  | `btoa-atob-`           | ~1    | task 012; btoa/atob                       | — (TODO)                                                                                                                         |
| 49  | `fetch-`               | ~5    | task 012; fetch                           | — (TODO)                                                                                                                         |
| 50  | `webcrypto-`           | ~10   | task 011 / webcrypto                      | — (TODO)                                                                                                                         |
| 51  | `whatwg-`              | ~5    | task 011 / webstreams                     | — (TODO)                                                                                                                         |
| 52  | `message-`             | ~3    | task 011 / message                        | — (TODO)                                                                                                                         |
| 53  | `navigator-`           | ~1    | task 012; navigator                       | — (TODO)                                                                                                                         |
| 54  | `performance-`         | ~1    | task 012; performance global              | — (TODO)                                                                                                                         |
| 55  | `permission-`          | ~5    | task 011 / permission (skip)              | — (skip)                                                                                                                         |

A few prefix groups (e.g. `test-`, `fixture-`, `common-`) are infrastructure
fixtures; they are loaded by the polyfill as `__nodeCommon` / `__nodeTmpdir`
rather than as Node tests.

## Slice template

For each row in the table above:

1. **Pick the cluster** and look at 2-3 representative fixtures in
   `tests/node/test/parallel/<prefix>-*.js`.
2. **Define a contract** by running the fixture under real `node` and
   recording observable behaviour. Write that contract as a single
   `tests/node-compat/stage-N/<name>.js` (mirror the Node semantics, not
   the polyfill).
3. **Implement** the smallest polyfill change in
   `crates/quench-node/polyfills/bootstrap.js` (or a host helper) that
   makes the stage pass.
4. **Run the up-stream fixtures** in the cluster via
   `tools/run-node-tests.sh` to confirm parity. Iterate until the cluster
   passes at the planned level.
5. **Commit** the focused stage + polyfill together with a single
   `git commit` per slice.

## Slicing rules

- One prefix per slice.
- A slice is "done" when: the focused stage passes, the
  `tools/check-focused-stages.sh` count does not regress, and the
  `tools/measure-node-tests.sh <prefix>` rate matches or exceeds the
  target for that prefix.
- Cross-prefix dependencies (e.g. `dgram` depends on `net`) are tracked
  by pre-emptive slices: implement `net` first, then `dgram` reads from
  it.

## Done when

- `tools/measure-node-tests.sh tests/node/test/parallel` reports ≥ 95%.
- `tools/check-focused-stages.sh` reports 100% of registered stages
  pass.
- Every row in the table above is either Done or Skipped with a
  documented reason (host limitation, intentional API omission).

## Status

In progress. Clusters 1-4 are the next batch after task 009.

## Retrospective — stage 559

The existing cluster lifecycle polyfill already exposed the primary/worker
state and setup methods, so a direct contract probe found the missing surface
quickly: `SCHED_NONE` and `SCHED_RR`. Keeping this slice as a small final
bootstrap fragment avoided touching the Rust host and made the stage and lint
checks complete in one iteration. The next cluster slice should measure
representative upstream fixtures before adding more API surface.

Stage 560 extends that probe to the worker connection predicate exposed by
the upstream cluster fixtures.

Stage 561 compared the actual `Worker` prototype with Node and filled the
missing `isDead()` and `destroy()` methods without changing the host layer.

Stage 562 compared the module-level cluster properties and filled the missing
default `schedulingPolicy` value with the existing `SCHED_RR` constant.

Stage 563 found repeated `setupPrimary()` calls discarded prior settings. A
small wrapper now merges new options with the existing settings before using
the established setup implementation.

Stage 564 added the default `args`, `exec`, `execArgv`, and `silent` settings
for the first no-argument `setupPrimary()` call, matching the upstream
contract while retaining the cumulative merge behavior.

The audit also corrected stage 505’s setup-event assertion to check the full
Node settings shape instead of the simulator’s former empty-settings shape.

Stage 565 compared the module aliases directly and restored Node’s identity
relationship between `setupMaster` and `setupPrimary`.

Stage 566 corrected `cluster.workers` from an array to Node’s object keyed by
worker ID, while retaining the simulator’s internal push and iteration paths.

Stage 567 made the worker-removal behavior an explicit contract: exiting
workers must disappear from the public collection. The cleanup adapter also
removes a worker synchronously on `kill()`, matching Node’s observable event
ordering.

Stage 568 filled the connected child-process IPC methods exposed through
`worker.process` after a cluster fork.

Stage 569 added the missing `exec`, `execFile`, `execSync`, and
`execFileSync` exports with callback and buffer contracts for the simulator.

Stage 570 added the standard spawn return-object fields and basic stdio
stream objects needed by child-process fixtures.

Stage 571 added the `spawn` and `close` lifecycle events while preserving the
existing exit event contract.

Stage 572 added the `ENOENT` spawn error shape for unresolvable commands,
including syscall, path, and spawn arguments.

Stage 573 added the shell-style exit code and direct `ENOENT` callback errors
for invalid `exec()` and `execFile()` commands.

Stage 574 added the synchronous `spawnSync()` `ENOENT` result shape for
invalid commands.

Stage 575 added thrown error contracts for invalid `execSync()` and
`execFileSync()` commands.

Stage 576 added synchronous type/value validation for the basic `spawn()`
command, arguments, and options forms.

Stage 577 exposed the public `child_process.ChildProcess` constructor and
aligned spawned-object `instanceof` behavior.

Stage 578 added the callable `_forkChild(fd, options)` export required by
Node’s child-process module surface.

Stage 579 added `on`, `once`, and `setEncoding` to spawned stdio streams so
child-process fixtures can attach normal stream listeners.

Stage 580 added simulated stdout/stderr `data`, `end`, and `close` delivery
for environment commands, including deterministic option-provided variables.

Stage 581 added the missing `ChildProcess.ref()` method alongside the
existing `unref()` method.

Stage 582 added Buffer output conversion for `exec({ encoding: "buffer" })`.

Stage 583 verified that simulated `env` children inherit `process.env` when
no explicit environment option is provided.

Stage 584 locked the Node event ordering invariant: `spawn` precedes stdio
and process lifecycle events.

Stage 585 added readable, writable, and destroyed state flags to simulated
stdio streams.

Stage 586 added basic `process.send()` callback and handle validation with
Node-compatible error codes.

Stage 587 extended those validation rules to `ChildProcess.send()` on forked
children.

Stage 588 aligned the default simulated fork exit status with Node’s normal
successful child exit.

Stage 589 added `ChildProcess.destroy()` and `ChildProcess[Symbol.dispose]()`
as chainable termination surfaces.

Stage 100 added a complete `fs.readdir` `withFileTypes` contract for sync,
callback, and promise calls, including file and directory predicates.

Stage 101 verified the internal `fs` flag parser across read, write, append,
exclusive, synchronous, and invalid flag forms.

Stage 102 verified numeric and octal-string modes for synchronous and
callback-based `fs.open` calls.

Stage 103 extended `fs.close` coverage to successful callback closure and
the `EBADF` result for a descriptor closed twice.

Stage 104 retained descriptor mode-mask coverage for `open` and `fchmod`,
confirming special mode bits do not leak into the public permission mask.

Stage 105 expanded promise-based `readdir` `Dirent` coverage to file and
directory predicates, parent paths, and cleanup of the temporary tree.

Stage 106 expanded directory-handle coverage to synchronous, callback, and
promise `opendir` reads and closes, including end-of-directory behavior.

Stage 107 expanded asynchronous symlink/readlink coverage with explicit file
type selection, round-trip target verification, and cleanup.

Stage 108 expanded symlink metadata coverage to sync and callback `stat`/
`lstat` calls, verifying link identity versus followed-target identity.

Stage 109 expanded promise symlink/readlink coverage with explicit file type,
target round-trip, promise `lstat`, and cleanup.

Stage 110 added sync, callback, and promise `fs.unlink` coverage, including
Node-compatible invalid path argument validation.

Stage 111 added hard-link coverage for sync, callback, and promise `fs.link`,
including invalid source and destination path validation.

Stage 112 formalized the existing `fs.read` options-object fixture, asserting
sync and callback byte results and cleaning up its descriptor and file.

Stage 113 formalized the Buffer/options callback form of `fs.read`, including
the Node-compatible `offset: null` default and identity of the result buffer.

Stage 114 formalized `fs.read` range and position validation, asserting
`ERR_OUT_OF_RANGE` and `ERR_INVALID_ARG_TYPE` results with cleanup.

Stage 115 expanded vectored reads to sync, callback, and promise `fs.readv`
forms, asserting byte counts, returned buffers, and cleanup.

Stage 116 formalized the write/read fixture and added sync vectored
`fs.writev` coverage with exact contents and cleanup.

Stage 117 formalized `fs.open` write-mode truncation, asserting stale content
is removed before writing and cleaning up the path.

Stage 118 formalized `Buffer.equals` coverage after vectored writes, asserting
both equal and unequal byte sequences with cleanup.

Stage 119 formalized invalid-buffer validation for sync and callback
`fs.writev`, including descriptor and path cleanup.

Stage 120 formalized promise-based vectored `fs.readv`, asserting byte count,
returned-buffer identity, contents, and cleanup.

Stage 121 formalized promise-based vectored `fs.writev`, asserting bytes
written, exact file contents, and cleanup.

Stage 122 formalized the options-object callback form of `fs.write`,
asserting selected-byte count, output contents, and cleanup.

Stage 123 formalized `util.promisify` callback conversion and resolved-value
behavior.

Stage 124 formalized promise file-handle reads, asserting byte count, buffer
identity/content, close, and cleanup.

Stage 125 formalized fd-based `fs.writeFileSync`, asserting exact output and
cleanup after closing the descriptor.

Stage 126 formalized typed-array `fs.writeFileSync` input, asserting decoded
contents and cleanup.

Stage 127 formalized DataView `fs.writeFileSync` input, asserting decoded
contents and cleanup.

Stage 128 formalized the upstream common `getArrayBufferViews` helper,
asserting multiple views with matching byte lengths.

Stage 129 formalized `fs.writeFileSync` encoding and append-flag options,
asserting append and overwrite results with cleanup.

Stage 130 formalized `fs.writeFileSync` flush validation and successful flush
writing, asserting invalid types, output, and cleanup.

Stage 131 formalized fd-based callback `fs.appendFile`, asserting success,
exact appended contents, and cleanup.

Stage 132 formalized promise-based fd `fs.promises.appendFile`, asserting the
same observable result and cleaning up the temporary file.

Stage 133 formalized synchronous `fs.ftruncate`, asserting successive size
changes through an open descriptor and cleaning up the temporary file.

Stage 134 formalized callback `fs.truncate` length validation, asserting the
Node error code for each invalid type and cleaning up the temporary file.

Stage 135 formalized synchronous `fs.truncate` fractional-length validation,
asserting `ERR_OUT_OF_RANGE` and cleaning up the temporary file.

Stage 136 formalized hexadecimal and base64 `fs.readFileSync` encodings,
asserting their exact decoded-string results and cleaning up the temporary file.

Stage 137 formalized the synchronous `fs.readFileSync` buffer option,
asserting returned contents and preservation of an untouched buffer byte.

Stage 138 formalized `fs.readFileSync` with the `a+` flag, asserting creation
of a missing file, its empty decoded result, and cleanup.

Stage 139 formalized default `fs.writeFileSync` permission bits, comparing the
created mode with the process umask and cleaning up the temporary file.

Stage 140 formalized promise file-handle writes with a source offset and
explicit file position, asserting byte count, resulting contents, and cleanup.

Stage 141 formalized promise file-handle vectored writes and reads, asserting
both byte counts, reconstructed contents, and cleanup.

Stage 142 formalized promise file-handle truncation, asserting the resulting
contents after shrinking an open file and cleaning up the temporary file.

Stage 143 formalized file-handle metadata, `sync`, and `datasync`, asserting
file size/type metadata and cleanup after closing the handle.

Stage 144 formalized file-handle chmod and closed-handle behavior, asserting
permission bits, `EBADF`, and cleanup.

Stage 145 formalized promise file-handle `writeFile` and `readFile`, asserting
round-trip contents and cleanup after closing the handle.

Stage 146 formalized promise file-handle `appendFile`, asserting appended
contents after closing the handle and cleaning up the temporary file.

Stage 147 formalized native synchronous `fs.realpath` behavior by comparing it
with the standard synchronous resolver and cleaning up the temporary file.

Stage 148 formalized UTF-8 and Buffer encoding options for synchronous
`fs.realpath`, asserting equivalent path results and cleanup.

Stage 149 formalized the upstream common fixtures helper, asserting that its
`fixturesDir` points to the Node test fixture tree.

Stage 150 formalized promise `fs.realpath` with string and Buffer encodings,
asserting equivalent results and cleaning up the temporary file.

Stage 151 formalized promise fd `fstat` and `fchmod`, asserting metadata,
permission bits, descriptor closure, and cleanup.

Stage 152 formalized invalid numeric paths for synchronous and callback
`fs.access`, asserting Node's `ERR_INVALID_ARG_TYPE` diagnostics.

Stage 153 formalized promise `fs.chmod`, asserting updated permission bits and
cleaning up the temporary file.

Stage 154 formalized promise copy/rename/unlink mutations, asserting preserved
contents after the rename and cleaning up all temporary files.

Stage 155 formalized promise directory removal, asserting the directory no
longer exists after `fs.promises.rmdir`.

Stage 156 formalized promise `mkdtemp`, asserting prefix and directory
semantics before removing the temporary directory.

Stage 157 formalized promise `lstat` on a symbolic link, asserting link
metadata rather than target metadata and cleaning up both entries.

Stage 158 formalized synchronous, callback, and promise hard-link creation,
asserting shared contents and cleaning up both links and the source.

Stage 159 formalized invalid source and target argument validation for
`fs.linkSync`, asserting Node's `ERR_INVALID_ARG_TYPE` diagnostics.

Stage 160 formalized UTF-8 and Buffer encoding options for synchronous
`fs.readlink`, asserting equivalent link targets and cleaning up both entries.

Stage 161 formalized the upstream symlink-capability helper, asserting that
the harness environment supports symbolic-link fixtures.

Stage 162 formalized invalid target argument validation for `fs.symlinkSync`,
asserting Node's `ERR_INVALID_ARG_TYPE` diagnostics.

Stage 163 formalized invalid symlink-type validation, asserting Node's
`ERR_INVALID_ARG_VALUE` diagnostic.

Stage 164 formalized basic promise file-handle `writeFile`, asserting the
round-trip contents after closing the handle and cleaning up the file.

Stage 165 formalized iterable input to promise file-handle `writeFile`,
asserting concatenated string/Buffer contents and cleanup.

Stage 166 formalized async-iterable input to promise file-handle `writeFile`,
asserting concatenated contents and cleanup.

Stage 167 formalized invalid value validation for promise file-handle
`writeFile`, asserting `ERR_INVALID_ARG_TYPE` and cleaning up the file.

Stage 168 formalized encoding options for promise file-handle `writeFile`,
asserting the decoded Latin-1 round trip and cleanup.

Stage 169 formalized file-handle pull iteration through the upstream stream
helpers, asserting text and byte lengths and cleaning up the file.

Stage 170 formalized pull `start`, `limit`, and `chunkSize` options, asserting
the selected slice and cleaning up the file.

Stage 171 formalized pull locking, post-read position state, and closed-handle
errors, asserting `ERR_INVALID_STATE` and cleanup.

Stage 172 formalized pull transforms and pre-aborted signals, asserting
transformed text, `AbortError`, and cleanup.

Stage 173 formalized invalid file-handle pull options, asserting Node's
argument-type/range diagnostics and cleanup.

Stage 174 formalized `stream/iter` and `zlib/iter` pull transforms, asserting
compressed/decompressed round-trip text and cleanup.

Stage 175 formalized pull batch chunking, asserting batch shape, chunk size,
batch count, and cleanup.

Stage 176 formalized promise file-handle `readFile`, asserting decoded file
contents and the `AbortError` from an already-aborted signal.

Stage 177 formalized promise file-handle `write`, asserting string writes and
`ERR_INVALID_ARG_TYPE` for invalid data values.

Stage 178 formalized callback `realpath`, asserting default and null options,
successful results, and the error callback contract for missing paths.

Stage 179 formalized path namespace, parse, and format behavior, asserting
POSIX/Win32 exports and invalid argument diagnostics.

Stage 180 formalized Win32 path parsing, formatting, and basename behavior,
asserting Windows roots, separators, extensions, and trailing separators.

Stage 181 formalized `util.format` primitives and placeholders, asserting
numeric conversions, missing/extra arguments, symbols, JSON fallback, and
negative zero.

Stage 182 formalized `util.inspect` defaults and `formatWithOptions`, asserting
the default numeric-separator setting and option-aware formatting.

Stage 183 formalized `util.format` BigInt conversions, asserting `%d`, `%i`,
and `%f` output for BigInt values.

Stage 184 formalized numeric `util.format` conversions for symbols, asserting
the `NaN` result for decimal, integer, and floating-point placeholders.

Stage 185 formalized numeric separators in `util.format` and
`formatWithOptions`, asserting separator-aware Number and BigInt output,
including large values.

Stage 186 formalized `%s` formatting for objects, asserting abbreviated nested
arrays and custom `toString` conversion.

Stage 187 formalized Buffer hexadecimal writes and decoding, asserting write
length, zero-filled remainder, and truncation at invalid hex characters.

Stage 188 formalized Buffer `includes`, asserting string and byte searches,
offset behavior, empty-value handling, and encoding-aware matching.

Stage 189 formalized Buffer `includes` validation, asserting
`ERR_INVALID_ARG_TYPE` for unsupported search values.

Stage 190 formalized Buffer double-precision read/write operations, asserting
big- and little-endian round trips, byte order, and Infinity handling.

Stage 191 formalized Buffer unsigned integer read/write operations, asserting
endianness, round trips, and `ERR_BUFFER_OUT_OF_BOUNDS` diagnostics.

Stage 192 formalized variable-width Buffer unsigned integer operations,
asserting six-byte big- and little-endian round trips.

Stage 193 formalized Buffer `Uint`/`UInt` method aliases, asserting that the
modern spellings preserve the original method identity.

Stage 194 formalized validation for variable-width Buffer reads, asserting
argument-type and range errors for invalid byte lengths.

Stage 195 formalized Buffer signed integer operations, asserting fixed-width
big/little-endian round trips and a variable-width signed round trip.

Stage 196 formalized Buffer single-precision float operations, asserting
big/little-endian round trips and write return offsets.

Stage 197 formalized Buffer slicing, callable construction, and comparison,
asserting negative bounds, callable `Buffer`, and `Buffer.compare` behavior.

Stage 198 formalized Buffer copy ranges and overlap semantics, asserting copied
counts, selected source ranges, and safe self-overlapping copies.

Stage 199 formalized Buffer concatenation, asserting Uint8Array inputs,
explicit truncation limits, and zero-filled output when the input list is empty.

Stage 200 formalized Buffer fill values, asserting encoded string repetition,
range bounds, and numeric byte filling.

Stage 201 formalized Buffer forward and reverse index searches, asserting
string offsets, byte searches, and missing-value results.

Stage 202 formalized Buffer JSON serialization, asserting the Node Buffer JSON
shape and restoration through `Buffer.from`.

Stage 203 formalized Buffer equality, asserting typed-array compatibility,
mismatch detection, and `ERR_INVALID_ARG_TYPE` validation.

Stage 204 formalized Buffer byte-length calculation, asserting UTF-8,
UTF-16LE, typed-array, and ArrayBuffer sizing.

Stage 205 formalized Buffer string writes, asserting offsets, byte counts,
UTF-8 output, and UTF-16LE encoding.

Stage 206 formalized Buffer instance and static comparison, asserting typed
array compatibility, equality, and lexicographic ordering.

Stage 207 formalized Buffer factory constructors, asserting `Buffer.of`
coercion and `allocUnsafeSlow` sizing.

Stage 208 formalized Buffer endian swap operations, asserting `swap16`,
`swap32`, and `swap64` in-place byte reordering.

Stage 209 formalized Buffer BigInt read/write operations, asserting signed
and unsigned 64-bit values in both byte orders.

Stage 210 formalized Buffer ArrayBuffer views, asserting offset/length
construction and shared mutable memory with the backing buffer.

Stage 211 formalized Buffer `isEncoding`, accepting every supported encoding
(both case variants) and rejecting invalid inputs.

Stage 212 formalized Buffer `copyBytesFrom`, asserting element-size-aware
byte copying with offset/length bounds and defaults.

Stage 213 formalized the `vm.runInNewContext` shim, evaluating code in a
fresh context with the supplied sandbox globals.

Stage 214 formalized Buffer `from` string coercion, asserting boxed strings
and `Symbol.toPrimitive` sources.

Stage 215 formalized Buffer `from` argument validation, asserting
`ERR_INVALID_ARG_TYPE` for invalid inputs and array-like construction.

Stage 216 formalized Buffer constants, asserting `kMaxLength` equals
`constants.MAX_LENGTH` and `MAX_STRING_LENGTH` presence.

Stage 217 formalized Buffer backing metadata, asserting `parent`/`buffer`
ArrayBuffer references, zero offset, and `poolSize` presence.

Stage 218 formalized Buffer `toString` encoding case-insensitivity, asserting
`BASE64URL` and `HEX` encodings.

Stage 219 formalized Buffer `write` encoding validation, asserting
`ERR_UNKNOWN_ENCODING` for unsupported encodings.

Stage 220 formalized Buffer `write` overload validation, asserting
`ERR_INVALID_ARG_TYPE` when a non-numeric offset is provided.

Stage 221 formalized Buffer `toString` offset/range clamping, asserting
end-bounds and negative-start clamping.

Stage 222 formalized Buffer `from` encoding conversions, asserting ascii
byte truncation and utf-16le round trips.

Stage 223 formalized Buffer base64 whitespace tolerance, asserting embedded
newlines and spaces are ignored during decoding.

Stage 224 formalized Buffer base64 invalid-input handling, asserting leading
invalid padding yields an empty Buffer.

Stage 225 formalized Buffer `inspect`, asserting the canonical `<Buffer ...>`
hex-hex rendering.

Stage 226 formalized Buffer `write` UTF-8 partial-character handling,
asserting multi-byte characters spanning the buffer boundary are truncated.

Stage 227 formalized Buffer lone-surrogate UTF-8 replacement, asserting
unpaired surrogates encode as the U+FFFD replacement character.

Stage 228 formalized Buffer `alloc` encoded fill values, asserting hex-string
fills repeat across the buffer and truncate at the end.

Stage 229 formalized Buffer `from` unknown-encoding validation, asserting
`ERR_UNKNOWN_ENCODING` for unsupported encodings.

Stage 230 formalized Buffer prototype metadata and alloc edge cases,
asserting `parent`/`offset` are absent on the prototype and negative
array-like lengths clamp to zero.

Stage 231 formalized `crypto.randomBytes` and `randomFillSync`, asserting
Uint8Array results, sizing, and in-place fill.

Stage 232 formalized Buffer `isAscii`/`isUtf8` predicates, asserting valid
ASCII/UTF-8 acceptance and rejection of non-ASCII and overlong sequences.

Stage 233 formalized `util` TextEncoder/TextDecoder exports, asserting
encode/decode round trips.

Stage 234 formalized `internal/errors` code classes, asserting instantiable
`ERR_OUT_OF_RANGE` as a RangeError.

Stage 235 formalized Buffer `fill` invalid hex validation, asserting
`ERR_INVALID_ARG_VALUE` for malformed hex fill strings.

Stage 236 formalized Buffer `fill` null coercion, asserting `null` fills
with zero bytes.

Stage 237 formalized Buffer `fill` range argument validation, asserting
`ERR_INVALID_ARG_TYPE` for non-numeric offset/length.

Stage 238 formalized the internal `arrayBufferViewHasBuffer` marker, asserting
small views are not backed while large views are.

Stage 239 formalized internal lazy arrayBuffer backing state, asserting
stable marker results across repeated checks.

Stage 240 formalized Buffer `allocUnsafeSlow` argument validation, asserting
`ERR_INVALID_ARG_TYPE` for non-numeric sizes and `ERR_OUT_OF_RANGE` for
negative sizes.

Stage 241 formalized Buffer `toString` end-range edge clamping, asserting a
negative end clamps to an empty slice.

Stage 242 formalized Buffer BigInt read/write alias parity, asserting
`readBigUInt64LE`/`readBigUint64LE` and `writeBigUInt64BE`/`writeBigUint64BE`
share implementations.

Stage 243 formalized Buffer display hooks, asserting `toLocaleString`
delegates to `toString` and the inspect custom symbol renders hex.

Stage 244 formalized the `internal/buffer` `utf8Write` export, asserting its
function presence.

Stage 245 formalized Buffer size fractional truncation, asserting `alloc`
and `allocUnsafe` truncate fractional sizes to integer lengths.

Stage 246 formalized Buffer `indexOf` UCS-2 odd-offset handling, asserting
an odd byte offset in ucs2 search yields no match.

Stage 247 formalized Buffer `indexOf` encoding overload, asserting a hex
string with an explicit encoding argument is matched.

Stage 248 formalized `util.inspect` Buffer rendering, asserting the canonical
`<Buffer ...>` hex output.

Stage 249 formalized Buffer `INSPECT_MAX_BYTES` live limit, asserting the
truncated `<Buffer ... ... N more byte>` rendering.

Stage 250 formalized Buffer inspect own-property rendering, asserting
user-defined properties appear alongside the hex dump.

Stage 251 formalized `util.format` symbol numeric conversions, asserting a
symbol coerced to a numeric placeholder yields `NaN`.

Stage 252 formalized the internal `JSStream` binding shim, asserting an
instantiable stream with an `_externalStream` handle.

Stage 253 formalized `util.types` basic predicates, asserting `isDate`,
`isMap`, and `isArrayBufferView`.

Stage 254 formalized the full `util.types` predicate matrix, asserting each
predicate fires only for its own category across all value kinds.

Stage 255 formalized typed-array brand checks, asserting DataView and
TypedArray are distinguished by brand even under prototype spoofing.

Stage 256 formalized `util.types` vm module namespace and key checks,
asserting `isModuleNamespaceObject` and empty key-object/crypto-key results.

Stage 257 formalized Buffer ascii byte conversion, asserting high-bit
characters map to the exact low-byte sequence (verified against Node).

Stage 258 formalized Buffer detached-arraybuffer state validation, asserting
`ERR_INVALID_STATE` when operating on a transferred (detached) buffer.

Stage 259 formalized Buffer surrogate-pair UTF-8 encoding, asserting lone
surrogates map to U+FFFD and valid pairs to the 4-byte sequence, matching
`TextEncoder` byte-for-byte on long input.

Stage 260 formalized Buffer `compare` offset-selection, asserting explicit
target/start/end offsets and `ERR_INVALID_ARG_TYPE` for non-numeric offsets.

Stage 261 formalized Buffer `fill` forged-length bounds check, asserting
`ERR_BUFFER_OUT_OF_BOUNDS` when a forged `length` exceeds the backing store
(verified against Node).

Stage 262 formalized Buffer `copyBytesFrom` element-size bounds, asserting
byte-accurate copies from typed arrays and past-end clamping to empty.

Stage 263 formalized Buffer `concat` length and truncation, asserting the
`totalLength` cap, empty results, and Buffer typing.

Stage 264 formalized Buffer inspect limit and named-property rendering,
asserting the live `INSPECT_MAX_BYTES` truncation and own-property inspection.

The `INSPECT_MAX_BYTES` contract was aligned with real Node: `Buffer`
carries no such static (it is `undefined`), and the truncation limit is
controlled solely by the `require("buffer").INSPECT_MAX_BYTES` accessor
(verified against Node 26).

Stage 276 formalized `util.format` float and numeric edge cases, asserting
symbol/empty float coercion to `NaN` and negative-zero integer/string
rendering (verified against Node).

Stage 277 formalized `util.inspect` string and function rendering, asserting
quoted strings and named/anonymous function labels.

Stage 278 formalized the `node:test` options callback invocation, asserting
a test with options runs its callback.

Stage 279 formalized `url.format`, asserting empty-query preservation and
legacy URL-object serialization.

Stage 280 formalized `url.parse` component extraction, asserting protocol,
host, port, path, query, and hash fields plus `ERR_INVALID_ARG_TYPE` for
non-string input.

Stage 281 formalized `url.parse` error codes, asserting `ERR_INVALID_URL`,
`ERR_INVALID_ARG_VALUE`, and raw `URIError` propagation (verified against
Node).

Stage 282 formalized `url.parse` normalization, asserting host/auth
lowercasing, whitespace trimming, and backslash path handling.

Stage 283 formalized `querystring.parse`/`stringify` core behavior,
asserting repeated keys, null-prototype results, plus encoding, and
percent-encoding preservation (verified against Node).

Stage 284 formalized `querystring.stringify` object value coercion,
asserting Date/RegExp/function values stringify empty and booleans/BigInt
stringify (verified against Node).

Stage 285 formalized `querystring` encoder options and URI errors, asserting
custom `encodeURIComponent` and `ERR_INVALID_URI` for lone surrogates
(verified against Node).

Stage 286 formalized `querystring` numeric coercion and `maxKeys`, asserting
NaN/Infinity stringify empty, separator/equalizer overloads, and key caps.

Stage 287 formalized `querystring` decoding paths, asserting `unescapeBuffer`
plus/space handling, custom `decodeURIComponent`, and `maxKeys` (verified
against Node).

Stage 288 formalized `querystring` decode fallback and `unescape` override,
asserting the default decoder is used when `decodeURIComponent` throws and
that a monkey-patched `querystring.unescape` is honored.

Stage 289 formalized `querystring.unescape` malformed-escape handling,
asserting partial/odd `%` escapes pass through unchanged (verified against
Node).

Stage 290 formalized `querystring` upstream-complete behavior, asserting
array values, separator/equalizer overloads, and the `unescape` override.

Stage 291 formalized `util.format` rendering for null-prototype objects,
asserting the explicit null-prototype marker and property output.

Stage 292 formalized `util.format` rendering for class instances with a null
prototype, asserting the class name and null-prototype marker.

Stage 293 formalized `util.format` rendering for ordinary class instances,
asserting the class name and enumerable instance properties.

Stage 294 formalized `util.format` rendering for Array subclasses, asserting
the subclass name, sparse length, and enumerable properties.

Stage 295 formalized `util.format` Symbol.toPrimitive conversion, asserting
the string hint for `%s` and default coercion in concatenation.

Stage 296 formalized `util.format` rendering for Date and Symbol built-ins,
asserting Date inspection equivalence and Symbol descriptions.

Stage 297 formalized `%s` Date conversion in `util.format`, asserting the
canonical ISO timestamp string.

Stage 298 formalized `%o` object-string formatting, asserting quoted strings
and escaped apostrophes.

Stage 299 formalized POSIX path validation, asserting `ERR_INVALID_ARG_TYPE`
for invalid values passed to common path operations.

Stage 300 formalized Win32 path validation, asserting `ERR_INVALID_ARG_TYPE`
for invalid values passed to common Windows path operations.

Stage 301 formalized path `basename` suffix handling, asserting invalid suffix
validation and suffix removal for POSIX and Win32 namespaces.

Stage 302 formalized path namespace separators and delimiters, asserting the
platform-specific values exposed by POSIX and Win32 paths.

Stage 303 formalized upstream-complete path normalization and joining,
asserting POSIX/Win32 parent traversal and basename suffix behavior.

Stage 304 formalized Win32 literal path normalization and basename behavior,
asserting escaped separators and suffix removal.

Stage 305 formalized Win32 parsing of slash-rooted paths, asserting the root,
directory, basename, extension, and name fields.

Stage 306 formalized POSIX parsing of trailing separators and `./`, asserting
the normalized root, directory, basename, extension, and name fields.

Stage 307 formalized POSIX `dirname` behavior for root paths, trailing
separators, and relative paths.

Stage 308 formalized POSIX `dirname` handling of trailing separators and
all-slash paths.

Stage 309 formalized Win32 UNC path parsing, asserting the server/share root,
directory, basename, extension, and name fields.

Stage 310 formalized OS uptime and process priority APIs, asserting numeric
uptime/priority values and priority mutation.

Stage 311 formalized the positive OS uptime contract, asserting a numeric
value greater than zero.

Stage 312 formalized OS host information, memory/load metrics, and loopback
network interfaces.

Stage 313 formalized OS user information, asserting identity fields and
Buffer-encoded username/shell results.

Stage 314 formalized `os.devNull` and `availableParallelism`, asserting the
null-device path and a positive numeric CPU-parallelism result.

Stage 315 formalized numeric coercion of OS metric functions, asserting their
callable results convert to non-NaN numbers.

Stage 316 formalized the upstream-complete OS core metrics contract, asserting
numeric identity, uptime, memory, priority, and parallelism results.

Stage 317 formalized `common.mustCallAtLeast`, asserting callbacks may exceed
their minimum invocation count while recording the actual count.

Stage 318 formalized `os.tmpdir` environment precedence, asserting trailing
separator normalization and TMPDIR/TMP/TEMP fallback ordering.

Stage 319 formalized POSIX `os.tmpdir` slash preservation, asserting backslash
and root slash values are retained as POSIX environment paths.

Stage 320 formalized string-returning OS functions, asserting template-string
coercion matches each function's direct result.

Stage 321 formalized `os.totalmem` numeric coercion, asserting the function
reference converts to the same number returned by invoking it.

Stage 322 formalized the upstream-complete OS core contract, asserting the
combined tmpdir, CPU, parallelism, memory, user information, and devNull APIs.

Stage 323 formalized core `util` compatibility helpers, covering legacy array
and object extension checks plus USV conversion and VT control stripping.

Stage 324 formalized `util` validation behavior, distinguishing native errors
from forged prototypes and preserving invalid-argument error metadata.

Stage 325 formalized the internal IPC channel-closed error, preserving its
Node error code and native-error classification.

Stage 326 formalized the upstream-complete `util` contract, combining legacy
helpers, USV conversion, VT stripping, and native-error recognition.

Stage 327 formalized `util.format` trailing-argument joining and object-format
inspection behavior.

Stage 328 formalized detailed `%o` object inspection, including function
metadata, circular references, and multiline formatting.

Stage 329 formalized `%O` shallow object inspection, including unquoted string
values and function rendering.

Stage 330 formalized nested `%o` inspection, preserving array length metadata
and nested function reference rendering.

Stage 331 formalized nested object `%o` inspection, preserving nested property
structure and function reference rendering.

Stage 332 formalized `util.format` Error handling, returning an Error's stack
when it is the sole formatting argument.

Stage 333 formalized `%c` CSS directive handling, discarding the style value
while preserving subsequent arguments.

Stage 334 formalized `%j` circular JSON handling, rendering circular values as
`[Circular]` instead of throwing.

Stage 335 formalized `util.formatWithOptions` color handling across mixed
primitive arguments.

Stage 336 formalized `util.formatWithOptions` compact inspection, preserving
compact array rendering and nested object elision.

Stage 337 formalized `util.format` SharedArrayBuffer inspection, including its
byte contents and byte-length metadata.

Stage 338 formalized custom Error formatting, preserving a custom error name
and message in the single-argument representation.

Stage 339 formalized `util.formatWithOptions` option validation, preserving
`ERR_INVALID_ARG_TYPE` for non-object option values.

Stage 340 formalized a non-debug common fixture regression check for
`util.types.isDate`.

Stage 341 formalized non-TTY process stream flags, ensuring stdout and stderr
report `isTTY === false` in the harness.

Stage 342 formalized `assert.AssertionError` inheritance and assertion throwing
behavior.

Stage 343 formalized basic `vm` context creation and expression evaluation.

Stage 344 formalized `assert` handling of an Error object as the assertion
message, preserving the original Error instance when thrown.

Stage 345 formalized `assert.throws` returning the thrown error and matching
its constructor.

Stage 346 formalized `assert.throws` matching an expected error object’s name
and message properties.

Stage 347 formalized assertion message formatting for long strict comparisons
and operator metadata for loose inequality failures.

Stage 348 formalized `assert.throws` rejecting a thrown error with the wrong
constructor and wrapping that mismatch in an `AssertionError`.

Stage 349 formalized generated assertion metadata, including `ERR_ASSERTION`
and the `generatedMessage` marker.

Stage 350 formalized `assert.doesNotThrow` failure handling, including the
assertion error code and operation metadata.

Stage 351 formalized `assert.throws` regular-expression and predicate-function
validators for thrown errors.

Stage 352 formalized assertion failures when a thrown value does not satisfy a
regular-expression validator.

Stage 353 formalized strict reference mismatch handling for distinct object
values and its `AssertionError` metadata.

Stage 354 formalized the `assert.throws` missing-exception failure, including
its standard message and operation metadata.

Stage 355 formalized strict comparison of distinct Error objects and the
resulting assertion failure.

Stage 356 formalized custom `assert.throws` missing-exception message
formatting with an expected constructor and user-provided message.

Stage 357 formalized Buffer signed and unsigned numeric reads across little- and
big-endian encodings.

Stage 358 formalized Buffer write range validation and `ERR_OUT_OF_RANGE`
metadata for negative offsets.

Stage 359 formalized Buffer UTF-8 and UCS-2 write encodings with corresponding
round-trip and byte-layout checks.

Stage 360 formalized Buffer instance and static comparison methods across
Buffer and Uint8Array inputs.

Stage 361 formalized Buffer fill encoding validation and its invalid-argument
error metadata.

Stage 362 formalized Buffer binary sharing, subarray/slice mutation behavior,
copy isolation, and concatenation.

Stage 363 formalized binary Buffer round-trips through synchronous filesystem
write and read APIs.

Stage 364 formalized positioned synchronous filesystem reads and writes using
Buffer arguments and offsets.

Stage 365 formalized appending binary Buffer data with synchronous filesystem
write options.

Stage 366 formalized timer timeout scheduling, cancellation, and microtask
ordering.

Stage 367 formalized repeating interval callbacks, cancellation after a fixed
count, and microtask observation of the final count.

Stage 368 formalized timer callback arguments plus `setImmediate` handle
cancellation.

Stage 369 formalized `process.nextTick` callback argument forwarding.

Stage 370 formalized writable stream backpressure, high-water-mark behavior,
and asynchronous drain notification.

Stage 371 formalized readable stream pause/resume flow control and deferred
delivery of remaining values.

Stage 372 formalized Transform stream chunk processing, output emission, and
write-callback ordering.

Stage 373 formalized piped readable/writable backpressure and drain-driven
delivery of subsequent chunks.

Stage 374 formalized SHA-256 hashing of Buffer input and hexadecimal digest
output.

Stage 375 formalized cryptographic random byte generation and offset filling
with in-place Buffer return semantics.

Stage 376 formalized filesystem ENOENT errors, including open syscall metadata
for missing paths.

Stage 377 formalized filesystem path argument validation and invalid-argument
error metadata.

Stage 378 formalized invalid path validation for synchronous filesystem writes.

Stage 379 formalized unknown filesystem encoding validation and its typed error
metadata.

Stage 380 formalized HMAC generation from Buffer key and message inputs with a
hex digest.

Stage 381 formalized binary crypto digest output as a Uint8Array-compatible
Buffer with the expected SHA-256 length.

Stage 382 formalized writable stream completion ordering, with `finish` emitted
before the `end` callback.

Stage 383 formalized fresh runtime state for each fixture, preventing globals
from leaking between separate stage files.

Stage 384 formalized crypto module bootstrap with the `randomUUID` API exposed
when the module is loaded.

Stage 385 formalized lazy crypto initialization, ensuring module access flips
the initialization state only when the crypto implementation is requested.

Stage 386 formalized lazy stream initialization, ensuring the stream module
loads its implementation only when requested.

Stage 387 formalized `fs.closeSync` rejection of unknown descriptors with
`EBADF` and the `close` syscall metadata.

Stage 388 formalized lazy URL compatibility initialization and exposure of
`url.parse` on module access.

Stage 389 formalized lazy OS compatibility initialization and exposure of
`os.platform` on module access.

Stage 390 formalized asynchronous `fs.close` completion and descriptor release,
including subsequent `EBADF` behavior for the closed descriptor.

Stage 391 formalized synchronous and callback PBKDF2 derivation with SHA-256
vectors and Buffer output.

Stage 392 formalized synchronous PBKDF2 validation for invalid iteration counts
and missing digest arguments with Node error codes.

Stage 393 formalized lazy querystring compatibility initialization and
`querystring.stringify` availability on module access.

Stage 394 formalized experimental `node:stream/iter` gating through the
`--experimental-stream-iter` runtime flag and unknown-builtin errors otherwise.

Stage 395 formalized crypto capability enumeration, advertising supported
hashes while returning cipher capability arrays.

Stage 396 formalized constant-time buffer comparison results and the Node length
error for unequal `timingSafeEqual` inputs.

Stage 397 formalized synchronous and callback `crypto.randomInt` range behavior,
including rejection of empty ranges.

Stage 398 formalized asynchronous `crypto.randomFill` offset and length bounds,
in-place Buffer mutation, and callback return identity.

Stage 399 formalized base64 digest encoding for SHA-256 hashes and HMAC values.

Stage 400 formalized hash and HMAC update decoding for explicit input encodings
such as hexadecimal strings.

Stage 401 formalized `timers/promises.setTimeout` delay handling and resolved
value propagation.

Stage 402 formalized `perf_hooks.performance` monotonic timing, numeric origins,
and `toJSON` availability.

Stage 403 formalized user-timing marks, measures, entry metadata, and mark
clearing through `perf_hooks.performance`.

Stage 404 formalized performance entry queries by name and type, aggregate entry
listing, and named measure clearing.

Stage 405 formalized `timers/promises.setInterval` async iteration and repeated
resolved values.

Stage 406 formalized `timers/promises` AbortSignal cancellation with Node’s
`AbortError` name and `ABORT_ERR` code.

Stage 407 formalized AbortSignal cancellation for async timer intervals, with
no value yielded after cancellation and matching abort metadata.

Stage 408 formalized AbortSignal cancellation for `timers/promises.setImmediate`
with matching abort metadata.

Stage 409 formalized process platform and architecture metadata plus the shapes
of `memoryUsage` and `resourceUsage` results.

Stage 410 formalized `process.binding` rejection of unknown internal modules
with Node’s “No such module” error wording.

Stage 411 formalized `process.getBuiltinModule` resolution for known modules
and undefined results for unknown names.

Stage 412 formalized `perf_hooks.timerify` wrapper error propagation and
observer lifecycle handling.

Stage 413 formalized `FileHandle` position tracking across `read` and
`readFile`, including continuation from the current offset.

Stage 414 formalized `FileHandle.readFile` encoding conversion for remaining
file content.

Stage 415 formalized `fs.readFileSync` support for open file descriptors and
string encoding conversion.

Stage 416 formalized synchronous `fs.appendFile` data validation and its
`ERR_INVALID_ARG_TYPE` error code.

Stage 417 formalized binary Buffer preservation through `fs.appendFileSync`
without text encoding conversion.

Stage 418 formalized `fs.appendFileSync` decoding of hex and base64 string
inputs through the encoding option.

Stage 419 formalized `fs.statSync` missing-path errors with `ENOENT` and `stat`
syscall metadata.

Stage 420 formalized independent state branching through `hash.copy` and
`hmac.copy`.

Stage 421 formalized rejection of updates and repeated digests after crypto
finalization with `ERR_CRYPTO_HASH_FINALIZED`.

Stage 422 formalized `fs.WriteStream` open/end/close ordering and encoded output
content.

Stage 423 formalized `fs.ReadStream` data delivery, close lifecycle, and Buffer
chunk concatenation.

Stage 424 formalized ReadStream text encoding options and `bytesRead` tracking.

Stage 425 formalized synchronous rejection of inverted ReadStream ranges with
`ERR_OUT_OF_RANGE`.

Stage 426 formalized WriteStream encoding options, byte accounting, and output
content validation.

Stage 427 formalized ReadStream descriptor cleanup, leaving `fd` null after
close.

Stage 428 formalized WriteStream descriptor cleanup, leaving `fd` null after
close.

Stage 429 formalized process exit event delivery, listener ordering, and the
default zero exit code.

Stage 430 formalized stream `autoClose: false` behavior, retaining the file
descriptor after stream close for caller-managed cleanup.

Stage 431 formalized WriteStream `autoClose: false` descriptor retention and
caller-managed closure.

Stage 432 formalized stream destruction, error propagation, close emission, and
the `destroyed` state transition.

Stage 433 repaired the sequential fixture ledger with process `nextTick`
ordering before promise callbacks.

Stage 434 formalized synchronous `process.nextTick` callback validation with
`ERR_INVALID_ARG_TYPE`.

Stage 435 formalized synchronous callback validation for timeout, immediate,
and interval scheduling APIs.

Stage 436 formalized timer handle reference control through `ref`, `unref`, and
`hasRef`, including cleared-handle state.

Stage 437 formalized readable stream pause/resume observability through
`isPaused`.

Stage 438 formalized writable backpressure reporting through
`writableNeedDrain` and drain-state clearing.

Stage 439 formalized readable end state and writable ended/finished completion
flags.

Stage 440 formalized readable and writable state flags clearing after stream
destruction.

Stage 441 formalized nested writable corking and uncorking through
`writableCorked`.

Stage 442 formalized buffering of corked writes and their emission on uncork.

Stage 443 formalized append-mode WriteStream behavior without truncating
existing file content.

Stage 444 formalized Readable `push` chunk delivery, end signaling, and the
`push(null)` return state.

Stage 445 formalized Readable `unshift` chunk delivery and ordering.

Stage 446 formalized Readable `read` size handling, FIFO buffering, and empty
queue results.

Stage 447 formalized EOF delivery after buffered data is consumed and the
paused Readable is resumed.

Stage 448 formalized Readable iterator consumption without replaying chunks
already delivered to data listeners.

Stage 449 formalized Readable EOF insertion through `unshift(null)` while
preserving buffered body data.

Stage 450 formalized default Readable Buffer queueing and complete queue drain.

Stage 451 formalized transition to flowing mode after data listeners attach,
including queued data and EOF delivery.

Stage 452 formalized `resume()` draining queued data from a paused Readable.

Stage 453 formalized writable length accounting and absence of spurious drain
events for writes below the high-water mark.

Stage 454 formalized chainable writable `end()` behavior and finish emission.

Stage 455 formalized rejection of writes after end with
`ERR_STREAM_WRITE_AFTER_END` callback metadata.

Stage 456 formalized rejection of Readable pushes after EOF with
`ERR_STREAM_PUSH_AFTER_EOF`.

Stage 457 formalized rejection of Readable unshifts after end with
`ERR_STREAM_UNSHIFT_AFTER_END_EVENT`.

Stage 458 formalized `Readable.read(0)` returning null without consuming queued
data.

Stage 459 formalized `readableEnded` remaining false while data is delivered
and becoming true only after the Readable emits `end`.

Stage 460 formalized the `readable` event exposing data queued while the
Readable is paused.

Stage 461 formalized a late `readable` listener receiving data that was queued
before the listener was attached.

Stage 462 formalized `readableLength` tracking queued bytes through partial and
complete buffer reads.

Stage 463 formalized `readableFlowing` transitions from the initial null state
to flowing, paused, and flowing again.

Stage 464 formalized readable and writable object-mode flags and their byte-mode
defaults.

Stage 465 formalized `destroy(error, callback)` preserving the original error
for the callback and emitting `close`.

Stage 466 formalized the same destroy callback error and close behavior for
Writable streams.

Stage 467 formalized `Readable.from()` constructing an object-mode Readable.

Stage 468 formalized timer handles exposing chainable `ref()` and `unref()`
state, including cleared-handle state.

Stage 469 formalized chainable `refresh()` on timeout and interval handles.

Stage 470 formalized `refresh()` reactivating a cleared timeout exactly once.

Stage 471 formalized `refresh()` reactivating a cleared interval exactly once.

Stage 472 formalized chainable Readable `setEncoding()`, exposed encoding
state, queued-buffer decoding, and unknown-encoding validation.

Stage 473 formalized Readable encoding decoding data delivered through flowing
`data` events.

Stage 474 formalized Writable writes after destruction returning false and
reporting `ERR_STREAM_DESTROYED`.

Stage 475 formalized Readable pushes after destruction throwing
`ERR_STREAM_DESTROYED`.

Stage 476 formalized Writable `writableLength` counting UTF-8 byte length for
multibyte string writes.

Stage 477 formalized Readable reads combining queued buffer chunks while
preserving remaining data.

Stage 478 formalized Readable string pushes being converted to byte data with
correct queue length and partial reads.

Stage 479 formalized `fs.readFile()` rejecting calls without a callback using
`ERR_INVALID_ARG_TYPE`.

Stage 480 formalized `fs.mkdtemp()` requiring a callback with
`ERR_INVALID_ARG_TYPE`.

Stage 481 formalized `fs.mkdtempSync()` rejecting non-string prefixes with
`ERR_INVALID_ARG_TYPE`.

Stage 482 formalized async `fs.mkdtemp()` rejecting non-string prefixes with
`ERR_INVALID_ARG_TYPE` before invoking its callback.

Stage 483 formalized sync and async `fs.mkdtemp()` rejecting invalid options
types with `ERR_INVALID_ARG_TYPE`.

Stage 484 formalized HMAC digest rejecting unknown output encodings with
`ERR_UNKNOWN_ENCODING`.

Stage 485 formalized hash digest rejecting unknown output encodings with
`ERR_UNKNOWN_ENCODING`.

Stage 486 formalized finalized Hash and Hmac contexts rejecting `copy()` with
`ERR_CRYPTO_HASH_FINALIZED`.

Stage 487 formalized `crypto.randomBytes()` rejecting negative, fractional, and
non-function callback arguments.

Stage 488 formalized `crypto.randomFillSync()` validating buffer input and
offset/length ranges.

Stage 489 formalized `querystring.stringify()` rejecting lone surrogates with
the expected URI error.

Stage 490 formalized querystring parsing and unescaping preserving Unicode
characters in literal and percent-encoded forms.

Stage 491 formalized `querystring.parse()` consulting the module's writable
`unescape` hook.

Stage 492 formalized `internal/event_target` exposing its weak-handler symbol.

Stage 493 formalized the async_hooks surface for execution resources, numeric
execution IDs, and chainable hook enable/disable methods.

Stage 494 formalized an in-process HTTP server/client exchange, response
headers, UTF-8 decoding, and JSON body delivery.

Stage 495 formalized async resource state propagation into timer callbacks.

Stage 496 formalized `AsyncResource.bind()` preserving function arity,
receiver, async ID, and callback validation.

Stage 497 formalized Buffer legacy encoding slice and write methods being
available on the prototype.

Stage 498 formalized the public Buffer prototype surface and Buffer instance
identity without legacy underscored methods.

Stage 499 formalized the Buffer inspect hook formatting generic typed arrays.

Stage 500 formalized Buffer float writes rejecting fractional and out-of-range
offsets with precise `ERR_OUT_OF_RANGE` messages.

Stage 501 formalized child-process exit events reporting the numeric exit code
and null signal for normal termination.

Stage 502 formalized forked child IPC sends returning boolean backpressure
signals as the send queue fills.

Stage 503 formalized child processes launched with eval code reporting normal
exit completion.

Stage 504 formalized cluster primary state and worker online/disconnect
lifecycle controls.

Stage 505 formalized cluster setup events exposing configured exec, args,
execArgv, and silent settings.

Stage 506 formalized the shared child-process fixture helper validating exit
status, signal, and stderr predicates.

Stage 507 formalized cluster worker state transitions, listening metadata, and
SIGTERM exit reporting.

Stage 508 formalized cluster fork environment overrides and IPC message
delivery.

Stage 509 formalized cluster and worker disconnect events, exited-after-
disconnect state, and clean worker exit metadata.

Stage 510 formalized cluster worker termination by signal and corresponding
exit/disconnect state.

The stage harness now has a fast path for parallel execution: `tools/check-all-
tests.sh` uses `cargo-nextest` for Cargo tests when available and a parallel
Node-stage runner for the CLI-driven API fixtures. This preserves the existing
serial checker as the deterministic fallback while allowing larger fixture
sets to use available CPU cores.
