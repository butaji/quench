# Implement Node-compatible native timers

## Goal

Replace the current microtask-based timer substitutions with correct timer scheduling while preserving Node APIs.

## Scope

- Implement `setTimeout`, `setInterval`, `setImmediate`, and cancellation behavior.
- Preserve `process.nextTick` ordering relative to promise jobs and timers.
- Ensure callbacks, arguments, exceptions, and timer handles match expected Node behavior.
- Add focused stages before changing broader scheduling behavior.

## Done when

- Timer compatibility stages pass consistently.
- Pending jobs are drained correctly by the existing Rust harness.

## Status

Initial delayed timeout and cancellation behavior is implemented and covered by
`tests/node-compat/stage-366/timer-timeout.js`. Native interval scheduling and
full nextTick/promise/timer ordering remain in progress.
