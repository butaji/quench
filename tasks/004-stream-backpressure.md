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
Experimental `node:stream/iter` gating is covered by
`tests/node-compat/stage-394/stream-iter-flag.js`.
Shared stream `destroy()` state and error/close events are covered by
`tests/node-compat/stage-432/stream-destroy.js`.
Readable pause state introspection is covered by
`tests/node-compat/stage-437/stream-is-paused.js`.
