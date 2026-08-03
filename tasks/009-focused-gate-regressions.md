# Fix focused stage gate regressions

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
