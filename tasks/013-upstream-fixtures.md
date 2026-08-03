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

| #  | Prefix                       | Count   | Module / domain                          | Existing stage(s)                |
| -- | ---------------------------- | ------- | ---------------------------------------- | -------------------------------- |
| 1  | `cluster-`                   |  ~95    | task 009 next slice; cluster / child IPC | 504, 505, 506, 507, 508, 509, 510, 559, 560, 561, 562, 563, 564, 565, 566 (workers object) |
| 2  | `child-process-`             |  ~125   | task 011 / child_process; fork/exec/stdio| 501, 502, 503                    |
| 3  | `http-`                      |  ~250   | task 011 / http; server, client, agent   | 494                              |
| 4  | `http2-`                     |  ~60    | task 011 / http2; session / stream       | — (TODO)                         |
| 5  | `https-`                     |  ~30    | task 011 / https; TLS over loopback      | — (TODO)                         |
| 6  | `net-`                       |  ~80    | task 011 / net; TCP, Server, Socket      | 502 (subset)                     |
| 7  | `dgram-`                     |  ~90    | task 011 / dgram; UDP                    | — (TODO)                         |
| 8  | `dns-`                       |  ~40    | task 011 / dns; lookup, resolver         | — (TODO)                         |
| 9  | `tls-`                       |  ~30    | task 011 / tls; TLSSocket, Server        | — (TODO)                         |
| 10 | `fs-`                        |  ~400   | task 011 / fs; stream + op + watch       | many; gaps                       |
| 11 | `fs-promises-`               |  ~30    | task 011 / fs/promises                   | — (TODO)                         |
| 12 | `buffer-`                    |  ~110   | task 001; data + encoding                | many                             |
| 13 | `stream-`                    |  ~50    | task 004; pipeline / finished            | many                             |
| 14 | `stream-web-`                |  ~25    | task 011 / stream/web; WHATWG streams    | — (TODO)                         |
| 15 | `crypto-`                    |  ~250   | task 005; cipher, sign, key, pqc, hkdf   | 374-421, 484-488                 |
| 16 | `async-hooks-`               |  ~50    | task 006; execution resource, init       | 493, 495, 496                    |
| 17 | `async-local-storage-`       |  ~15    | task 011 / async_hooks                   | — (TODO)                         |
| 18 | `worker-`                    |  ~70    | task 011 / worker_threads (host)         | — (TODO)                         |
| 19 | `events-`                    |  ~25    | events; abort listener, custom, on async  | — (TODO)                         |
| 20 | `eventtarget-`               |  ~10    | task 012; global EventTarget             | — (TODO)                         |
| 21 | `diagnostics-channel-`       |  ~50    | task 011 / diagnostics_channel           | — (TODO)                         |
| 22 | `readline-`                  |  ~30    | task 011 / readline; Interface           | — (TODO)                         |
| 23 | `repl-`                      |  ~25    | task 011 / repl; minimal                 | — (TODO)                         |
| 24 | `tty-`                       |  ~15    | task 011 / tty                           | — (TODO)                         |
| 25 | `assert-`                    |  ~25    | task 011 / assert; async, deep           | — (TODO)                         |
| 26 | `console-`                   |  ~25    | task 011 / console                       | — (TODO)                         |
| 27 | `url-`                       |  ~25    | url; WHATWG, fileURL                     | — (TODO)                         |
| 28 | `querystring-`               |  ~10    | task 007; unicode / unescape             | 489-491                          |
| 29 | `path-`                      |  ~5     | task 009; posix/win32                    | 179 (corrected)                  |
| 30 | `util-`                      |  ~25    | util; promisify, parseArgs, styleText    | 181 (format)                     |
| 31 | `timers-`                    |  ~5     | task 003; order, unref, refresh          | 366-471                          |
| 32 | `perf-hooks-`                |  ~5     | task 003; timerify, observer             | 402-412                          |
| 33 | `process-`                   |  ~10    | task 011 / process; report, signal       | 409-411, 429                     |
| 34 | `vm-`                        |  ~25    | task 011 / vm; source module, compile     | — (TODO)                         |
| 35 | `domain-`                    |  ~30    | task 011 / domain                        | — (TODO)                         |
| 36 | `module-`                    |  ~10    | task 011 / module; builtin, createReq    | — (TODO)                         |
| 37 | `os-`                        |  ~5     | task 011 / os; userInfo, cpus           | — (TODO)                         |
| 38 | `zlib-`                      |  ~5     | task 011 / zlib                          | — (TODO)                         |
| 39 | `inspector-`                 |  ~10    | task 011 / inspector (skip on host)      | — (skip)                         |
| 40 | `trace-events-`              |  ~3     | task 011 / trace_events (skip)           | — (skip)                         |
| 41 | `wasi-`                      |  ~3     | task 011 / wasi (skip)                   | — (skip)                         |
| 42 | `punycode-`                  |  ~3     | task 011 / punycode (alias)              | — (TODO)                         |
| 43 | `v8-`                        |  ~5     | task 011 / v8                            | — (TODO)                         |
| 44 | `abortcontroller-`           |  ~10    | task 011 / abort (abort controller)      | — (TODO)                         |
| 45 | `abortsignal-`               |  ~5     | task 011 / abort (signal)                | — (TODO)                         |
| 46 | `blob-`                      |  ~5     | task 011 / blob; stream                  | — (TODO)                         |
| 47 | `broadcastchannel-`          |  ~3     | task 012 / BroadcastChannel              | — (TODO)                         |
| 48 | `btoa-atob-`                 |  ~1     | task 012; btoa/atob                      | — (TODO)                         |
| 49 | `fetch-`                     |  ~5     | task 012; fetch                          | — (TODO)                         |
| 50 | `webcrypto-`                 |  ~10    | task 011 / webcrypto                     | — (TODO)                         |
| 51 | `whatwg-`                    |  ~5     | task 011 / webstreams                    | — (TODO)                         |
| 52 | `message-`                   |  ~3     | task 011 / message                       | — (TODO)                         |
| 53 | `navigator-`                 |  ~1     | task 012; navigator                      | — (TODO)                         |
| 54 | `performance-`               |  ~1     | task 012; performance global             | — (TODO)                         |
| 55 | `permission-`                |  ~5     | task 011 / permission (skip)             | — (skip)                         |

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
