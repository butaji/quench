# Implement stream backpressure semantics

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

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
Readable encoding selection and validation are covered by
`tests/node-compat/stage-472/stream-set-encoding.js`.
Flowing readable encoding delivery is covered by
`tests/node-compat/stage-473/stream-encoding-flow.js`.
Writable write-after-destroy rejection is covered by
`tests/node-compat/stage-474/stream-write-after-destroy.js`.
Writable string backpressure uses encoded byte length in
`tests/node-compat/stage-476/stream-write-byte-length.js`.
Readable sized reads combine adjacent byte chunks in
`tests/node-compat/stage-477/stream-read-combine.js`.
Byte-mode string pushes are normalized in
`tests/node-compat/stage-478/stream-push-string.js`.
Readable push-after-destroy rejection is covered by
`tests/node-compat/stage-475/stream-push-after-destroy.js`.
Writable `drain` transition and callback-free length recovery are covered by
`tests/node-compat/stage-453/stream-drain-transition.js`.
Chainable writable `end()` behavior is covered by
`tests/node-compat/stage-454/stream-end-return.js`.
Writable write-after-end rejection is covered by
`tests/node-compat/stage-455/stream-write-after-end.js`.
Readable push-after-EOF rejection is covered by
`tests/node-compat/stage-456/stream-push-after-eof.js`.
Readable unshift-after-end rejection is covered by
`tests/node-compat/stage-457/stream-unshift-after-end.js`.
Non-positive readable `read()` sizes are covered by
`tests/node-compat/stage-458/stream-read-zero.js`.
Readable `readableEnded` timing is covered by
`tests/node-compat/stage-459/stream-readable-ended-timing.js`.
Pull-mode `readable` event delivery is covered by
`tests/node-compat/stage-460/stream-readable-event.js`.
Late pull-mode `readable` listeners are covered by
`tests/node-compat/stage-461/stream-readable-late-listener.js`.
Live readable queue length is covered by
`tests/node-compat/stage-462/stream-readable-length.js`.
Readable flowing-state transitions are covered by
`tests/node-compat/stage-463/stream-flowing-state.js`.
Readable and writable object-mode state is covered by
`tests/node-compat/stage-464/stream-object-mode.js`.
Readable destroy callbacks are covered by
`tests/node-compat/stage-465/stream-destroy-callback.js`.
Writable destroy callbacks are covered by
`tests/node-compat/stage-466/stream-writable-destroy-callback.js`.
`Readable.from()` object-mode state is covered by
`tests/node-compat/stage-467/stream-from-object-mode.js`.
Experimental `node:stream/iter` gating is covered by
`tests/node-compat/stage-394/stream-iter-flag.js`.
Stages 169–174 exercise filehandle pull APIs and require the experimental
`stream/iter` flag when invoked directly.
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
