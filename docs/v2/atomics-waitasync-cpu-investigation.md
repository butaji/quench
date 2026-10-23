# Atomics.waitAsync CPU investigation

## Finding

The roughly two seconds of CPU reported for each Test262
`waitAsync/no-spurious-wakeup-*` case is not caused by the shared backing's
`AsyncWait::poll()` implementation. These cases use
`$262.agent.getReportAsync()` from Test262's `harness/atomicsHelper.js`. That
helper polls `getReport()` with `setTimeout(loop, 1000)`. When the host does not
provide `setTimeout`, the harness fallback recursively chains Promise
microtasks while waiting for `Date.now()` to reach its deadline. The recursive
microtasks keep the runtime job queue busy and consume CPU.

The recorded measurement at
`evidence/conformance-engines-linux-2026-10-22/test262-no-jit/samples.jsonl.gz`
records approximately 1.96–1.99 seconds of user CPU and 2.11 seconds of wall
time for the affected cases. The source is in the Test262 corpus checkout at
`tests/test262/harness/atomicsHelper.js` (that corpus checkout is not present in
this worktree).

## Why a local Atomics change is not correct

`SharedBacking::wait_async()` already registers a waiter and returns a token;
it does not spin. `Vm::drain_jobs()` drains Promise jobs and polls shared async
waiters, then returns when the Promise job queue is empty. It has no macrotask
timer queue or host task wait boundary. Quench Node already owns a separate
timer task queue and pump, but `quench-test262::Test262RuntimeHost` does not use
that host.

Adding sleeps to Atomics would not fix the timer fallback and would couple
Atomics semantics to a harness poll. Replacing the harness timer globally
would hide the missing runtime/host scheduling contract. Making the existing
`run_jobs()` wait for timer callbacks would change its current microtask-drain
contract and could block embeddings on pending handles.

## Test262 host scheduling

The Test262 adapter now installs `setTimeout` and `clearTimeout` for async tests
only. It stores callback and argument roots with a deadline, waits for the next
timer after the current Promise jobs drain, then enqueues the callback through
the existing rooted-job API. It drains Promise jobs again before advancing to
another timer. The runtime `run_jobs()` microtask-checkpoint contract and
Quench Node's existing timer pump remain unchanged.

Shared async waits are still polled by the VM's internal job drain; the public
`Runtime::run_jobs()` API intentionally drains only JavaScript jobs and does not
advance host agent tasks such as `waitAsync` completion. A timer task alone is
therefore not a way to make a `waitAsync` timeout settle in an embedding. This
task changes no Atomics waiter representation. A future host scheduler that
waits on multiple event sources can replace periodic polling with a shared wake
notification, provided notify count/FIFO behavior, timeout ordering,
cancellation, GC roots, and realm ownership stay exact.

The focused in-host test covers timer macrotask and Promise microtask ordering.
The upstream async Atomics Test262 case is the behavioral and CPU gate for the
polling regression; it exercises Test262 agent reports as well as timers.

The Test262 timer queue is scoped to its async-test host adapter; it is not a
new global timer implementation for `Runtime` embeddings.

## Measurement

The exact Test262 case
`built-ins/Atomics/waitAsync/no-spurious-wakeup-no-operation.js` passes with the
production `run-test` binary. Two isolated runs measured 2.111–2.114 s wall,
0.000–0.0036 s user CPU, 0.0068–0.012 s system CPU, and 11,484 KiB maximum
RSS. The archived production no-JIT `run-all` sample for the same case passes
at 2.109 s wall, 1.990 s user CPU, 0.005 s system CPU, and 19,800 KiB maximum
RSS. This is approximately 99.8% lower user CPU, unchanged wall time, and 42%
lower peak RSS. The host timer now sleeps until its deadline instead of burning
CPU in recursive Promise microtasks.

The host scheduling unit test passes and checks that Promise reactions drain
before timer callbacks, and that reactions queued by a timer run afterward.
