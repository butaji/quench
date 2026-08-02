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
