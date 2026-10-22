# V8-v7 native-entry coverage diagnostic

This diagnostic establishes current JIT coverage for the four requested hot
V8-v7 workloads. It is not a score comparison. The source revision is
`5ca192dad1abaeaa653bc998440f3b21c8bcfe74`; the pinned V8-v7 corpus is
`64e1860c736c1b899708f4cd646721bc71e53d8b`. The production `quench-node`
binary SHA-256 is
`f8c83ab825bad852f1a1b9d67265282173aa7c873c717b7b096d2ae5c5b0ba16`.

The four scripts concatenate the pinned `base.js`, each stock fixture, and
the `RUNNER` constant from `quench-bench/src/main.rs`. Each was run once with
`QUENCH_JS_JIT_STATS=1 target/production/quench-node --jit <script>`. All
completed and emitted their stock V8-v7 Score line, so their benchmark
assertions passed. The JIT stats are captured beside the input scripts.

| Fixture | Native function entries | Native OSR region runs | Relevant coverage |
| --- | ---: | ---: | --- |
| Crypto | 3,195 | 4,730,536 | `nbits` compiled; hot `am3` loop executes natively |
| DeltaBlue | 35,055 | 0 | Three tiny four-instruction functions compiled |
| NavierStokes | 0 | 48,954 | Five loop regions compiled across `set_bnd`, `lin_solve`, `project`, `advect`, and `lin_solve2` |
| Richards | 0 | 0 | `runRichards` and `Scheduler` reject with `unsupported-lowering-shape-or-codegen-failure` |

Richards is the immediate coverage target. Its hot `Scheduler.schedule` loop
and task helpers use mutable object fields and indirect calls. Adding more
numeric loop patterns will not reach this code. The next compiler step should
therefore prioritize guarded field reads/writes and cached call targets in
general region denotations, with exact lane fallback at the first unsupported
instruction. This also targets the shared interpreter/JIT execution path's
dominant object and call costs; the no-JIT side still needs its own matched
measurement.

Concurrent release-feature compilation was active during these diagnostics.
The printed scores and wall times are not performance evidence and must not be
compared with peers. Only completion and the native-entry counters are used
here. This five-vCPU, 17-GiB Linux x86_64 container does not establish the
separate Linux/M4 qualification criterion.
