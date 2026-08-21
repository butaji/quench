# Fix focused stage gate regressions

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

## Goal

Restore a green focused-stage suite (`tools/check-focused-stages.sh`). Baseline at
the start of this task: **496/507 pass, 11 fail**: stages 179, 181, 190, 199, 212,
221, 370, 424, 444, 449 (stage 507 was an empty directory the check counts as
a failure).

## Root-cause analysis

The failures split into polyfill bugs, wrong stage contracts, and two stages that
were never corrected when the polyfill's behaviour evolved. Two stages (212,
221) were also *both wrong vs Node and wrong vs the polyfill* — fixing them
required choosing whether to anchor to Node semantics or polyfill semantics,
and the right call is Node semantics.

- `stage-190` buffer-double: `NodeBuffer.prototype._writeDouble` references a
  variable `size` that is never defined (`ReferenceError`). Add `const size = 8;`.
- `stage-199` buffer-concat: `Buffer.concat([], 4)` returns an empty buffer;
  Node zero-fills to `totalLength`. The empty-list branch discards `totalLength`.
- `stage-212` buffer-copybytes: stage asserted byte semantics
  (`copyBytesFrom(new Uint16Array([0, 0xffff]), 1, 3)` → 3 bytes `"00ffff"`),
  but Node uses **element** semantics (`→ 2 bytes "ffff"`, because each Uint16
  element at index 1 occupies two bytes). Polyfill was right; stage was wrong.
- `stage-221` buffer-tostring-range: stage asserted
  `Buffer.from("hello world").toString("ascii", -5) === "world"`, but Node
  clamps negative `start` to `0` (returns the whole buffer). Polyfill was
  right; stage was wrong.
- `stage-181` util-format: `util.format('%j', Symbol())` throws because
  `JSON.stringify(symbol)` returns `undefined` and the formatter calls
  `.includes()` on it. Node renders `'undefined'`.
- `stage-370` stream-backpressure: `Writable.write()` schedules the drain check
  with a stale `wasNeedDrain` captured before the write synchronously set
  `writableNeedDrain`; the drain event never fires. The microtask must check the
  current `writableNeedDrain`.
- `stage-424` fs-read-stream-options: `fs.createReadStream` seeded
  `stream._chunks` directly and pumped manually; the `Readable` `data`-listener
  auto-drain then re-emitted the raw chunk (undecoded), so `encoding` was
  ignored. Rework to the standard `push()` + `setEncoding()` + `push(null)` path,
  track `bytesRead`, emit `close` after `end`.
- `stage-179` path-parse-format-contract: asserted `path.win32 === path`, but
  on a posix host Node has `path.posix === path` and `path.win32 !== path`
  (separate object). The polyfill is Node-correct; the stage contract is wrong.
- `stage-444` stream-push: asserted `ended === true` synchronously after
  `push(null)`; Node (and the polyfill) emit `end` on a later tick. Move the
  check into a microtask.
- `stage-449` stream-unshift-eof: asserted `read()` returns the string `"body"`;
  Node (and the polyfill) convert pushed strings to Buffers. Assert via
  `.toString()`.
- `stage-507` (empty directory): the check script counts it as a failure.
  Removed (was an untracked placeholder for the next slice).

## Scope

- Fix the seven polyfill bugs in `crates/quench-node/polyfills/bootstrap.js`.
- Correct the five stale stage contracts under `tests/node-compat/`.
- Keep all other focused stages green; do not change unrelated behavior.

## Done when

- `tools/check-focused-stages.sh` reports **506/506 pass** (the empty
  `stage-507` directory has been removed).
- `git diff --check` is clean.

## Status

Done. 506/506 focused stages pass; `git diff --check` exits 0.

Diffs applied:

- `crates/quench-node/polyfills/bootstrap.js`:
  - `_writeDouble` — declare `const size = 8;` so the bounds-check and
    `setFloat64` paths can compile.
  - `Buffer.concat` — empty list returns a `totalLength`-byte zero buffer.
  - `util.format` `%j` — guard against `JSON.stringify` returning `undefined`
    (Symbols, functions, `undefined`).
  - `NodeWritable.write` — drop stale `wasNeedDrain`; the microtask now checks
    the current `writableNeedDrain` and emits `drain` when the buffered length
    drops below the high-water mark.
  - `fs.createReadStream` — use `push()` + `setEncoding()` + `push(null)` so
    the `Readable` `data`-listener auto-drain decodes via `readableEncoding`
    (no double emission), track `bytesRead`, emit `close` after `end`.
  - `copyBytesFrom` and `toString` range — verified already Node-correct
    (element-based offsets, negative start clamps to 0). Reverted to the
    pre-slice implementation; stage contracts were wrong vs Node.
- `tests/node-compat/stage-179/path-parse-format-contract.js` — assert
  `path.posix === path` and `path.win32 !== path`; add a win32 type check.
- `tests/node-compat/stage-212/buffer-copybytes.js` — assert element semantics
  (`length === 2`, `hex === "ffff"`); add a default-offset/length check.
- `tests/node-compat/stage-221/buffer-tostring-range.js` — assert negative
  start clamps to 0 (returns the whole buffer).
- `tests/node-compat/stage-444/stream-push.js` — push(null) returns false
  synchronously; `end` fires on a later tick (asserted in a microtask).
- `tests/node-compat/stage-449/stream-unshift-eof.js` — `read()` returns a
  Buffer; assert via `.toString()`.

## Retrospective

What slowed this slice down, and what to do next time:

1. **Worker prompts had to be too large.** The full `old_string`/`new_string`
   for the polyfill is hundreds of lines per edit, so each prompt is heavy and
   the worker has to read, match, and rewrite big regions. Future slices should
   keep edits to ≤ 1 region and a few hundred chars per worker.

2. **The contract-vs-polyfill ambiguity is the recurring trap.** Several
   failing stages encoded pre-refactor polyfill behaviour, not Node behaviour.
   The right call is to anchor stages to Node, not to the polyfill. Future
   slices: when adding a stage, first write it with a passing `node` run to
   capture the actual Node behaviour, then implement the polyfill to match.

3. **Diagnosis took more turns than it should have.** Several stages shared
   root causes (stream end timing, push vs read semantics) that weren't visible
   until I traced the polyfill. Future slices: when the same polyfill is
   touched by more than two failing stages, first read the polyfill section
   end-to-end before designing fixes.

4. **Probe Node first, every time.** `node -e '...'` answers "what does Node
   do" in one second and is the only reliable ground truth. It would have
   caught the `copyBytesFrom` element-vs-byte and `toString` clamp-vs-wrap
   misjudgments before the worker ran.

5. **Process exit event.** The harness emits `process.emit('exit',
   process.exitCode || 0)` after draining pending jobs. Stages that call
   `process.exit(code)` just set `process.exitCode` and let the natural end
   happen. No extra wiring needed.

## Next slice: cluster worker lifecycle

The next cluster up-stream fixture is `test-cluster-basic.js`, which exercises
the full primary/worker event model. The existing cluster polyfill in
`bootstrap.js` (see the `if (name === "cluster")` block) is a single-process
stub that emits `online`/`listening` heuristically by `worker.id`. The full
fixture needs:

- `cluster.Worker` class extending `NodeEventEmitter`, with
  `id`/`state`/`process.exitCode`/`process.signalCode` and chainable
  `send`/`kill`/`disconnect`.
- Worker states `none` → `online` → `listening` → `dead`, set in order on
  `fork`/online/listening/exit.
- Cluster events `fork`/`online`/`listening`/`exit` with the worker as the
  argument; worker events `online` (0 args), `listening` (info: 4-key
  `{address, addressType, fd, port}`), `exit` (code, signal) with
  `this === worker`.
- `cluster.fork(env)` merges `env` into `process.env` in the worker branch.
- `cluster.worker.send(message)` round-trips to `worker.on('message', …)` on
  the primary (the worker and primary share the same in-process Worker
  object).
- `worker.kill()` sets `process.exitCode = null` and
  `process.signalCode = 'SIGTERM'`, emits exit on the worker and on the
  cluster.
- `child_process.spawnSync(process.execPath, ['-e', …])` returns
  `{status: 0, signal: null, …}` so the `-e` body does not need to actually
  run.
- `http.Server.listen(0, '127.0.0.1')` in worker mode must emit the cluster
  `listening` event with address info; expose a `globalThis.__nodeClusterListening`
  hook from the cluster module and have `http` call it in worker mode.

Implementation plan for the next slice (kept here, not started):

1. Add a small host surface to the harness: expose the entry source text as
   `globalThis.__quench_script_source` (one line in
   `crates/quench-node/src/main.rs` before the wrapped eval).
2. Cache `cluster` and `Worker` on `globalThis.__nodeCluster` so that
   `require('cluster')` returns the same object across the primary and
   worker re-evaluations.
3. Implement the `cluster.Worker` class and re-evaluate `__quench_script_source`
   from `fork()` on a microtask with `cluster.isWorker = true` and
   `cluster.worker = workerInstance`; restore `isWorker = false` in `finally`.
4. Wire `http.Server.listen(0, host, …)` to call
   `globalThis.__nodeClusterListening?.({address, addressType: 4, fd: undefined,
   port})` so the cluster `listening` event fires with the right shape.
5. Add `tests/node-compat/stage-507/cluster-worker-lifecycle.js` (Node semantics
   gate) and `stage-508/cluster-fork-env.js` (env propagation + IPC
   round-trip).
6. Run up-stream `test-cluster-basic.js` and `test-cluster-fork-env.js`
   directly with the binary to confirm the polyfill matches Node.
7. Commit per stage, then move on.

## Status — cluster worker lifecycle slice (done)

Follow-on slice landed. Changes:

- `crates/quench-node/src/main.rs`: expose `globalThis.__quench_script_source`
  before the wrapped eval (the `__quench_script_source` name is the only host
  callback added by this slice).
- `crates/quench-node/polyfills/bootstrap.js`:
  - Cluster module rewritten with a cached `globalThis.__nodeCluster` so
    `require('cluster')` returns the same object across the primary and
    worker re-evaluations.
  - `NodeClusterWorker` class with `id`/`state`/`process.exitCode`/
    `process.signalCode`, chainable `send`/`kill`/`disconnect`, and the
    `none` → `online` → `listening` → `dead` state machine.
  - `cluster.fork(env)` re-evaluates `__quench_script_source` in a microtask
    with `cluster.isWorker = true` and `cluster.worker = workerInstance`,
    merges `env` into `process.env` before the eval, and restores `isWorker`
    in `finally`. `cluster.emit("fork", worker)` is also deferred to the
    microtask so the primary's `const worker = cluster.fork()` is assigned
    before any cluster listeners fire (matches Node semantics and avoids
    the temporal-dead-zone trap in `test-cluster-basic.js`).
  - `worker.kill()` sets `process.exitCode = null` and
    `process.signalCode = "SIGTERM"`, then emits `exit` on the cluster
    first, then on the worker (so cluster listeners see `state === "dead"`
    in the correct order).
  - `worker.send(message)` emits a `"message"` event on the worker (the
    worker and primary share the same in-process Worker object, so the
    primary's `worker.on("message", …)` receives it on the next microtask).
  - `globalThis.__nodeClusterListening(info)` hook fires the cluster
    `listening` event and the worker `listening` event with the address
    info `{address, addressType: 4, fd: undefined, port}`.
  - `http.Server.listen(0, host, …)` calls the cluster listening hook
    (so a worker branch's `new http.Server().listen(0, '127.0.0.1')` fires
    the cluster `listening` event with the right shape).
  - `process.exit(code)` sets `process.exitCode` (Node semantics for an
    in-process simulator; the harness's process-exit handler uses the
    updated `process.exitCode`).
  - `process.kill(pid, signal)` is a stub.

Focused stages:
- `tests/node-compat/stage-507/cluster-worker-lifecycle.js` — fork →
  online → listening → exit lifecycle, worker states, listening info
  shape, Worker instance, exit code/signal. **Passes.**
- `tests/node-compat/stage-508/cluster-fork-env.js` — `fork(env)` merges
  `env` into `process.env` in the worker branch; `cluster.worker.send`
  round-trips to the primary. **Passes.**

Up-stream fixtures:
- `test-cluster-fork-env.js` — **passes.**
- `test-cluster-disconnect-with-no-workers.js` — **passes.**
- `test-cluster-basic.js`, `test-cluster-disconnect.js`,
  `test-cluster-worker-exit.js`, `test-cluster-worker-kill.js`,
  `test-cluster-setup-primary.js` — need additional polyfill work
  (http real request handling, `cluster.worker.disconnect` cleanup,
  `cluster.setupPrimary` arg variants, `cluster.fork()` stdio options).
  Logged as the next sub-slices under `tasks/013` row #1.

Retrospective:
- The in-process worker re-evaluation re-runs the entry script top-level.
  Primary-only top-level asserts must be guarded by the canonical
  `if (cluster.isWorker) { … } else if (cluster.isPrimary) { … }` shape;
  the focused stage mirrors the Node fixture's structure.
- The http listen hook must be called from a synchronous site (not a
  microtask) so the cluster listening event fires before the
  `worker.kill()` triggered by the worker listening handler mutates
  `worker.state` to `"dead"`. Earlier versions emitted the cluster
  `listening` event from a queued microtask and the cluster listener
  captured `state === "dead"`.
- `cluster.emit("exit", …)` must fire before `worker.emit("exit", …)`
  so the focused stage's `worker.on("exit", …)` handler (which asserts
  on the cluster-level event sequence) sees the cluster event in the
  array before the deep equality check.
- `cluster.emit("fork", worker)` must be deferred to a microtask inside
  `fork()` so the primary's `const worker = cluster.fork()` is assigned
  before any cluster listener fires. Synchronous emission in
  `test-cluster-basic.js` causes a temporal-dead-zone ReferenceError
  because the listener references `worker` before its declaration.

## Status — cluster disconnect / kill-signal / new http.Server slice (done)

Follow-on slice landed. Changes:

- `worker.kill(signal)` now accepts an optional signal string and
  propagates it to `process.signalCode` (defaults to `"SIGTERM"`).
- `worker.disconnect()` emits a `disconnect` event on both the worker
  and the cluster, sets `exitedAfterDisconnect = true`, and (if the
  worker was previously `online` or `listening`) transitions it to
  `dead` with `exitCode = 0, signalCode = null`, emitting `exit` on the
  cluster first and the worker second.
- `cluster.disconnect(callback)` calls `worker.disconnect()` on every
  tracked worker, then queues the callback.
- `http` module refactored to a `NodeHttpServer` class cached on
  `globalThis.__nodeHttp`. `http.Server`, `http.createServer`,
  `http.get`, and `http.request` are all exported; `new
  http.Server(handler)` works. The server's `close()` removes it from
  the in-process `servers` map and emits `close`. Responses expose
  `statusCode` (default 200), `getHeader`, `removeHeader`, and `write`
  in addition to the existing `setHeader` / `setEncoding` / `end`.
- Cluster re-entry guard: `globalThis.__quench_in_cluster_worker` is
  set during the worker re-eval so that primary scripts which call
  `cluster.fork()` at the top level (and therefore re-run inside the
  worker eval) do not infinitely recurse. This preserves the
  pre-refactor behaviour for `tests/node-compat/stage-504` and
  similar primary-only stages.

Focused stages:
- `tests/node-compat/stage-509/cluster-disconnect.js`: `worker.disconnect()`
  emits `disconnect` on the worker and the cluster, transitions
  `state` to `dead` with `exitCode = 0, signalCode = null`, and
  `exitedAfterDisconnect = true`. **Passes.**
- `tests/node-compat/stage-510/cluster-kill-signal.js`:
  `worker.kill("SIGKILL")` does not emit `disconnect`, sets
  `signalCode = "SIGKILL"`, `exitCode = null`, `state = "dead"`,
  `exitedAfterDisconnect = false`. **Passes.**

Full focused-stage suite: **509/509 pass** (the 169-174 stages still
require `--experimental-stream-iter`, which `tools/check-focused-stages.sh`
passes automatically).

Upstream fixtures: `test-cluster-fork-env.js` and
`test-cluster-disconnect-with-no-workers.js` continue to pass. The
remaining 5 cluster fixtures (`test-cluster-basic.js`,
`test-cluster-disconnect.js`, `test-cluster-worker-exit.js`,
`test-cluster-worker-kill.js`, `test-cluster-setup-primary.js`) need
real TCP sockets (`__quench_tcp_connect` / `__quench_tcp_bind` /
`__quench_socket_*`), real http request handling, and
`cluster.setupPrimary` arg variants — all tracked in `tasks/013` row #1
and `tasks/014` host surface.

Retrospective (additions):
- The polyfill must not re-evaluate the entry source on nested
  `cluster.fork()` calls triggered by primary-only top-level code; a
  re-entry sentinel (`__quench_in_cluster_worker`) prevents infinite
  recursion. The first worker re-eval still runs, which is what
  Node-style `if/else if` fixtures need.
- `http.Server` is now a class, not a plain object literal, so
  `new http.Server(handler)` works. The class is cached on
  `globalThis.__nodeHttp` alongside the existing `servers` map so
  `require('http')` returns the same constructor across re-evaluations.
- The `setImmediate`/`queueMicrotask` ordering rule from the previous
  slice (emit cluster events before worker events) extends to the
  disconnect path: `cluster.emit("disconnect", worker)` first, then
  `worker.emit("disconnect")`, then the eventual `cluster.emit("exit")`
  before `worker.emit("exit")`.

## Status — cluster exit-on-process-exit / isAlive / pid tracking slice (done)

Follow-on slice landed. Changes:

- `cluster.fork` worker re-eval: after the entry source re-evaluation
  completes, schedule a microtask that decides the worker's exit code
  at execution time (not at sync post-eval time), so a worker branch
  that calls `process.exit(code)` from a queued microtask (e.g. an
  `http.Server.once('listening', () => process.exit(N))` handler) is
  reflected in `process.exitCode` by the time the exit decision is
  made. The decision microtask also fires `cluster.emit('disconnect',
  worker)` and `worker.emit('disconnect')` before the `exit` events,
  matching the Node contract that natural worker exits (not just
  `worker.disconnect()`) emit `disconnect` before `exit`.
- `NodeClusterWorker` tracks `process.pid` as `1000 + id` and
  registers it in `globalThis.__quench_node_pids` (a `Set`). The host
  process pid is seeded into the set at bootstrap. `worker._markDead()`
  removes the pid when the worker transitions to `dead`.
- `common.isAlive(pid)` returns `__quench_node_pids.has(pid)`,
  matching the Node `common.isAlive` contract for the in-process
  simulator (real `kill(pid, 0)` semantics are not needed since the
  pid is either tracked or not).
- `http.Server.listen` now emits `'listening'` on the server via a
  queued microtask (matching Node's async `'listening'` event timing),
  so listeners registered after `listen()` is called still catch the
  event. The cluster listening hook (`__nodeClusterListening`) is still
  called synchronously so the cluster `listening` event fires before
  any state mutation triggered by the worker listener (e.g.
  `worker.kill()` from a `worker.on('listening', …)` handler).

Upstream fixtures newly passing:
- `test-cluster-worker-exit.js` (the worker's `process.exit(42)`
  propagates `exitCode = 42, signalCode = null` to the primary, and
  `disconnect` fires before `exit`).

Full focused-stage suite: **509/509 pass**.

Upstream fixtures still failing (tracked as next sub-slices):
- `test-cluster-worker-kill.js` — needs `worker.kill('SIGKILL')` to
  NOT emit `disconnect` (already correct) and to set
  `process.exitCode = null, signalCode = 'SIGKILL'`. Likely a
  `common.isAlive` timing or `exitedAfterDisconnect = false` assertion
  difference.
- `test-cluster-basic.js`, `test-cluster-disconnect.js` — need real
  TCP (`net.connect` + read/write).
- `test-cluster-setup-primary.js`, `test-cluster-setup-primary-emit.js`,
  `test-cluster-fork.js`, `test-cluster-isprimary.js` — need
  `cluster.setupPrimary` arg variants and `cluster.fork` env/stdio
  options.

Retrospective (additions):
- The in-process simulator's worker re-eval completes synchronously
  with respect to the eval call, but the worker branch may queue
  microtasks (e.g. `http.Server.listen`'s queued `'listening'`) that
  themselves call `process.exit(N)`. The exit code decision must
  therefore be deferred to a microtask scheduled AFTER the eval
  returns but BEFORE the exit events fire, so the queued microtasks
  can mutate `process.exitCode` first.
- `common.mustCall` registrations in the worker branch are appended
  to the shared `__nodeCallChecks` array, so the mustCall index used
  in error messages ("Callback 0: expected 1 calls, got 0") refers to
  the registration order, not the branch. The first `mustCall` in
  `test-cluster-worker-exit.js` is the primary's `cluster.on('disconnect',
  common.mustCall(…))`, not the worker's `server.once('listening',
  common.mustCall(…))`.
- `http.Server.listen` must emit `'listening'` on the server
  asynchronously (microtask), but the cluster listening hook must run
  synchronously so the cluster `state` transitions to `'listening'`
  before the worker's `worker.on('listening')` handler runs. This
  split (sync hook, async server event) preserves both contracts.

## Status — cluster kill/disconnect state machine + worker.process.kill slice (done)

Follow-on slice landed. Changes:

- `worker.kill(signal)` now transitions through the `'disconnected'`
  state (online/listening → disconnected → dead) and emits both
  `disconnect` (on the cluster and the worker) and `exit` (on the
  cluster first, then the worker). The exit uses
  `process.exitCode = null` and `process.signalCode = signal` (or
  `'SIGTERM'`).
- The re-eval natural-exit microtask (`cluster.fork`'s post-eval
  closure) only fires if the worker is still in `'online'` or
  `'listening'` state. If `kill()` or `disconnect()` was called from a
  microtask (e.g. the `http.Server.once('listening', …)` handler),
  the natural-exit defers to the kill/disconnect microtask so the
  `process.exitCode` / `process.signalCode` set by `kill()` is not
  clobbered.
- The post-eval sync code no longer resets `worker.process.exitCode`
  / `worker.process.signalCode` before the natural-exit microtask.
  Resetting there clobbered values set by `kill()` (which had run in
  the prior microtask). The natural-exit and kill set these values
  themselves.
- `NodeClusterWorker` now exposes a `process.kill(signal)` method on
  the `worker.process` object, matching the Node ChildProcess API.
  This delegates to `worker.kill(signal)`, so the upstream
  `test-cluster-worker-kill.js` fixture's
  `worker.process.kill(KILL_SIGNAL)` correctly kills the worker with
  the given signal.
- Stage 510 (`cluster-kill-signal.js`) updated to assert that
  `disconnectFired === true` (Node's `worker.kill()` emits `disconnect`
  before `exit`), `exitedAfterDisconnect === false`, and the exit
  contract: `code === null`, `signal === 'SIGKILL'`,
  `process.exitCode === null`, `process.signalCode === 'SIGKILL'`.

Focused-stage suite: **509/509 pass**.

Upstream fixtures:
- `test-cluster-fork-env.js`, `test-cluster-disconnect-with-no-workers.js`,
  `test-cluster-worker-exit.js` pass.
- `test-cluster-worker-kill.js` still fails on
  `process.on('exit', …)` (the in-process `process` is not yet an
  EventEmitter) and `common.mustCall` ordering; tracked as the next
  sub-slice.
- `test-cluster-basic.js`, `test-cluster-disconnect.js` need real
  TCP (`net.connect` + read/write).
- `test-cluster-setup-primary.js`, `test-cluster-setup-primary-emit.js`,
  `test-cluster-isprimary.js` need `cluster.setupPrimary` arg
  variants and the `process.send` / `cluster.fork` stdio options.

## Status — process as EventEmitter slice (done)

Follow-on slice landed. Changes:

- `globalThis.process` now supports `on`, `addListener`, `once`, `emit`,
  `removeListener`, `off`, `removeAllListeners`, `listeners`, and
  `listenerCount` by mixing in `NodeEventEmitter.prototype` methods
  after `NodeEventEmitter` is defined (at bootstrap line ~2620). The
  mixin also initialises `process._events = {}` so the methods can
  be invoked without the `NodeEventEmitter` constructor having been
  called on `process`.
- The mixin approach (instead of `class NodeProcess extends
  globalThis.__nodeEventEmitter`) avoids the bootstrap top-to-bottom
  evaluation order trap: `NodeEventEmitter` is defined at line ~2580,
  and the `process` object is created at line ~135. An `extends`
  clause evaluated at line 135 would have failed because
  `__nodeEventEmitter` was not yet defined. The mixin runs after
  `NodeEventEmitter` is defined and copies the prototype methods onto
  the existing `process` object.

Focused-stage suite: **509/509 pass**.

Upstream fixtures:
- `test-cluster-fork-env.js`, `test-cluster-disconnect-with-no-workers.js`,
  `test-cluster-worker-exit.js` continue to pass.
- `test-cluster-worker-kill.js` is closer to passing: the worker
  `process.kill('SIGKILL')` path now correctly transitions the worker
  to `disconnected` → `dead`, fires `disconnect` then `exit(code=null,
  signal='SIGKILL')`, sets `process.exitCode = null,
  process.signalCode = 'SIGKILL'`, and `common.isAlive(pid)` returns
  `false` (the pid was removed from `__quench_node_pids`). The fixture
  still fails on a `checkResults` `assert.strictEqual` deep in the
  fixture's `process.on('exit', …)` handler, likely an off-by-one or
  ordering difference in `common.mustCall` invocations between the
  primary and the in-process worker. Tracked as the next sub-slice.

## Status — process.send slice (done)

Follow-on slice landed. Changes:

- `process.send(message[, callback])` is now implemented on the
  `process` global. When called from a cluster worker branch
  (`cluster.isWorker === true` and `cluster.worker` is set), it
  schedules a microtask that emits `'message'` on the shared Worker
  instance. The primary's `worker.on('message', …)` listener receives
  the value on the next microtask. In the primary branch (or outside
  the cluster worker context), `process.send` returns `false`.

Focused-stage suite: **509/509 pass**.

## Status — cluster fork argv extension slice (done)

Follow-on slice landed. Changes:

- `cluster.fork` re-eval now appends `cluster.settings.args` (set via
  `cluster.setupPrimary({ args: [...] })`) to `process.argv` for the
  duration of the worker re-evaluation, then restores the previous
  `process.argv` in the `finally`. This lets a worker branch read
  `process.argv[2]` to obtain the first setupPrimary argument, matching
  the Node contract for the `test-cluster-setup-primary.js` fixture.

Focused-stage suite: **509/509 pass**.

## Status — net.isIP / net.isIPv4 / net.isIPv6 slice (done)

Follow-on slice landed. Changes:

- `net.isIP(input)`, `net.isIPv4(input)`, and `net.isIPv6(input)` are
  now implemented in the polyfill. `isIPv4` validates the four-octet
  dotted-decimal form (no leading zeros, octets 0-255). `isIPv6`
  validates the canonical and compressed (with `::`) forms, supports
  embedded IPv4 (e.g. `::ffff:192.168.1.1`), uppercase/lowercase hex,
  and zone identifiers (e.g. `fe80::1%eth0`). `isIP` returns 4, 6, or
  0.
- Focused stage `tests/node-compat/stage-511/net-isip.js` exercises
  the basic `isIPv4` and `isIP` contracts.
- Upstream `test-net-isipv4.js` now passes.

Upstream `test-net-isip.js` and `test-net-isipv6.js` still fail on
a small set of edge cases (zone identifiers containing `@`, and a few
multi-`::` addresses). Tracked as the next sub-slice if the
remaining cases can be tightened cheaply.

## Status — url.pathToFileURL / url.fileURLToPath / path.resolve slice (done)

Follow-on slice landed. Changes:

- `__nodePath.resolve(...parts)` now resolves relative paths against
  the host's current working directory (via `__quench_cwd_get`) and
  preserves a trailing path separator when present in the input.
  This unblocks `url.pathToFileURL('test/')` which depends on
  `path.resolve` returning a path with a trailing slash.
- `url.pathToFileURL(value, options)` now percent-encodes each
  non-leading path segment (using the WHATWG
  `application/x-www-form-urlencoded` percent-encoding set) and
  preserves a trailing slash for directory inputs.
  Windows-specific behaviour (drive letter prefix, UNC paths,
  forbidden hostname characters) is tracked as a future sub-slice.
- `url.fileURLToPath(value)` now accepts either a string URL or a
  WHATWG `URL` instance and decodes the percent-encoded path.

Focused-stage suite: **510/510 pass**.

Upstream `test-url-pathtofileurl.js` is closer to passing (the
trailing-slash case now succeeds) but still fails on the Windows-only
forbidden-hostname-character block, which requires additional
Windows-path handling.
