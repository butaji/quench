# Implement Node-compatible native timers

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

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
`tests/node-compat/stage-367/timer-interval.js`. Full nextTick/promise/timer
`tests/node-compat/stage-367/timer-interval.js`, and cancellable immediate
handles are covered by `tests/node-compat/stage-368/timer-handles.js`. Full
nextTick/promise/timer ordering remains in progress; callback argument
forwarding is covered by `tests/node-compat/stage-369/next-tick.js`.
Delayed `timers/promises.setTimeout()` value resolution is covered by
`tests/node-compat/stage-401/timers-promises-delay.js`.
Synchronous `process.nextTick()` callback validation is covered by
`tests/node-compat/stage-434/next-tick-validation.js`.
Synchronous callback validation for callback timers is covered by
`tests/node-compat/stage-435/timer-callback-validation.js`.
Timer handle ref/unref/hasRef methods are covered by
`tests/node-compat/stage-436/timer-handle-ref.js`.
Correct timer handle ref/unref state transitions are covered by
`tests/node-compat/stage-468/timer-handle-state.js`.
Timeout and interval refresh method contracts are covered by
`tests/node-compat/stage-469/timer-handle-refresh.js`.
Timeout refresh rescheduling behavior is covered by
`tests/node-compat/stage-470/timer-refresh-behavior.js`.
Interval refresh rescheduling behavior is covered by
`tests/node-compat/stage-471/interval-refresh-behavior.js`.
Minimal `perf_hooks.performance` timing APIs are covered by
`tests/node-compat/stage-402/perf-hooks-performance.js`.
User Timing marks and measures are covered by
`tests/node-compat/stage-403/perf-hooks-user-timing.js`.
Performance entry retrieval and clearing are covered by
`tests/node-compat/stage-404/perf-hooks-entries.js`.
`performance.timerify()` error propagation and observer lifecycle are covered
by `tests/node-compat/stage-412/perf-hooks-timerify.js`.
`timers/promises.setInterval()` async iteration is covered by
`tests/node-compat/stage-405/timers-promises-interval.js`.
Pre-aborted `timers/promises.setTimeout()` signals are covered by
`tests/node-compat/stage-406/timers-promises-abort.js`.
Pre-aborted `timers/promises.setInterval()` signals are covered by
`tests/node-compat/stage-407/timers-promises-interval-abort.js`.
Pre-aborted `timers/promises.setImmediate()` signals are covered by
`tests/node-compat/stage-408/timers-promises-immediate-abort.js`.
