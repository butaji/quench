# V8_v7 evidence

This is the performance decision record. `Observed` is measured, `Rejected`
is a completed A/B, and `Unknown` is not permission for a fast path.

## Current control

The native, fat-LTO, one-CGU, panic-abort artifact completed the full suite at
**102.0**: Richards 75.9, DeltaBlue 45.4, Crypto 74.7, RayTrace 88.5,
EarleyBoyer 98.3, RegExp 159, Splay 131, NavierStokes 252. All fixtures must
be valid before reporting an aggregate score.

| fixture | primary measured cost |
| --- | --- |
| EarleyBoyer | 90.32 CPU s, 1.848T instructions, 1.68 GB process-tree RSS |
| RegExp | 125.1M owned-word reads, 12.7M `Value` decodes, ownership/allocation leaves |
| Splay | about 500 MB RSS, comparable to Node; not the first reclamation target |

`/usr/bin/time -l` maximum RSS covers the runner process tree; never present it
as Quench-only RSS. Trace and profiler builds are diagnostic artifacts, never
score artifacts.

## Facts and constraints

- `TaggedValue(u64)` and compact `#[repr(u8)]` tags are already the execution
  transport. The expensive boundary is decode/clone/drop of broad owning
  `Value` objects.
- The ordinary recursive call path creates about 26M environments in Earley;
  almost all originate in `Environment::child_registers`/`build_registers`.
- RegExp's pattern cache is not the blocker: a trace measured 2.7M hits,
  108 misses, and sub-second engine/cache time.
- A captured-prefix workload means a non-capturing-only frame path misses the
  hot shape. Any new frame must preserve captures and retain ordinary fallback
  for escaping cells/closures, TDZ, `arguments`, direct `eval`, `with`, async,
  generators, suspension, host re-entry, throws, and tail transfers.
- No fixture/source/score/checksum recognition, AOT, JIT, unchecked access, or
  dropped statement completion. An effect-only call must still write the
  statement-completion value.

## Accepted direction

The next representation experiment is one immutable per-function layout and a
single contiguous owned-word activation window, with disjoint parameter,
lexical, and temporary views. It must replace the activation boundary—not pool
one wrapper—under proven facts and use the complete existing path on every
miss. Retain it only after a valid isolated Earley result improves by at least
2x, followed by a repeated valid full-suite run.

RegExp work is separate: remove reusable ownership/result materialization only
under ordinary built-in guards, with complete exec/proxy/override fallback.

## Completed experiments

| result | change | evidence / decision |
| --- | --- | --- |
| Accepted | ordinary global `RegExp[@@replace]` direct template scan after flags/`exec` guards | Node agrees on `lastIndex`; isolated runBlock0 273 in 13.99 s. Sticky/override paths fall back. |
| Accepted | two-field constructor permits harmless prototype methods | Boyer 5.21→2.50 CPU s, 92.3B→52.8B instructions; official Earley 98.3→105, RSS 1.68→1.58 GB. |
| Rejected | shared flattened-code view | unchanged Boyer instructions and CPU. |
| Rejected | remove outer continuation `maybe_grow` | Earley 110 vs 108 control: normal variation, not 2x. |
| Rejected | environment identity pool | Earley 109; pooling one wrapper leaves parameter/register materialization. |
| Rejected | execution `RegisterFile` pool | Earley 111; both activation vectors and the boundary remain. |
| Rejected | operand-window `CallW` transport | ordinary `f(1,2)` lost output; reverted. |
| Rejected | proven-slot resize elision | Earley 105, worse than control. |
| Rejected | immutable-code leaf/shape cache key | Earley 105; cache admission is not the primary limiter. |
| Rejected | force-inline compact dispatch | helper disappeared but Earley 111, not 2x. |
| Rejected | unique-array COW clone removal | DeltaBlue valid; RegExp still timed out in that partial run. |

## Evidence gate

For every experiment, record commit/diff, lockfile, toolchain/effective flags,
binary hash and DWARF identity, raw repeated per-fixture wall/RSS/instruction
samples, semantic counter deltas, and Node differential results. Control and
diagnostic lanes are separate. A hypothesis predicts a counter delta; reject
it if release wall time does not move, only instrumentation improves, or
compatibility differs in values, errors, descriptors, identity, ordering, exit
status, or host effects. Use assembly/IR only to explain an accepted or
rejected measurement, not as a score substitute.
