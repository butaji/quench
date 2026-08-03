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
