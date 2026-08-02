# Implement stream backpressure semantics

## Goal

Make Node-compatible streams preserve buffering, ordering, pause/resume, and backpressure behavior.

## Scope

- Inspect the existing streams polyfill and stages.
- Implement native queue primitives only where they reduce copying or repeated allocation.
- Cover `Readable`, `Writable`, `Duplex`, `Transform`, `write()`, `drain`, `pipe()`, `pause()`, and `resume()`.
- Preserve observable event ordering and error propagation.

## Done when

- Focused stream stages pass.
- Backpressure does not change public Node semantics.

## Status

Initial writable backpressure (`highWaterMark`, `writableLength`, `drain`) is
implemented and covered by `tests/node-compat/stage-370/stream-backpressure.js`.
Readable pause/resume and full transform/event ordering remain in progress.

Readable pause/resume queue preservation is covered by
`tests/node-compat/stage-371/stream-pause-resume.js`.
Transform output through `push()` is covered by
`tests/node-compat/stage-372/stream-transform.js`.
Pipe backpressure propagation is covered by
`tests/node-compat/stage-373/stream-pipe-backpressure.js`.
Deferred `finish` and `end` callback ordering is covered by
`tests/node-compat/stage-382/stream-end-order.js`.
Readable chunk delivery and end signaling through `push()` are covered by
`tests/node-compat/stage-444/stream-push.js`.
Readable front-buffer injection through `unshift()` is covered by
`tests/node-compat/stage-445/stream-unshift.js`.
Readable queued chunk consumption through `read()` is covered by
`tests/node-compat/stage-446/stream-read.js`.
EOF delivery after buffered readable data is covered by
`tests/node-compat/stage-447/stream-eof-order.js`.
Readable async-iterator consumption does not replay pumped chunks in
`tests/node-compat/stage-448/stream-iterator-consumption.js`.
Readable `unshift(null)` EOF ordering is covered by
`tests/node-compat/stage-449/stream-unshift-eof.js`.
Default readable buffering before a `data` listener is covered by
`tests/node-compat/stage-450/stream-default-buffer.js`.
Adding a `data` listener drains buffered readable data in
`tests/node-compat/stage-451/stream-flow-transition.js`.
Explicit `resume()` drains paused readable data in
`tests/node-compat/stage-452/stream-resume-queue.js`.
Experimental `node:stream/iter` gating is covered by
`tests/node-compat/stage-394/stream-iter-flag.js`.
Shared stream `destroy()` state and error/close events are covered by
`tests/node-compat/stage-432/stream-destroy.js`.
Readable pause state introspection is covered by
`tests/node-compat/stage-437/stream-is-paused.js`.
Writable `writableNeedDrain` transitions are covered by
`tests/node-compat/stage-438/stream-writable-need-drain.js`.
Readable and writable completion flags are covered by
`tests/node-compat/stage-439/stream-completion-state.js`.
Readable/writable lifecycle state flags are covered by
`tests/node-compat/stage-440/stream-readable-writable-state.js`.
Writable cork state is covered by
`tests/node-compat/stage-441/stream-cork.js`.
Deferred corked writes and final uncork delivery are covered by
`tests/node-compat/stage-442/stream-cork-buffer.js`.
