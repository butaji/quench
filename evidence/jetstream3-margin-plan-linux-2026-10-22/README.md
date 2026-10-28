# JetStream 3: plan to a 20% margin over Bun and Node (2026-10-22)

Host: Linux x86_64, 4 vCPU, 15 GiB (cloud container). Branch `v2-cloud-jet` at `08f3a15f`.

`roadmap.md` is the synthesis of six research dimensions: promise/async, Wasm and v128,
GC, parse/compile, interpreter core, and JIT. Every proposal was checked by an adversarial
verifier against the code and against the data in
`../jetstream3-nojit-vs-bun-default-linux-2026-10-21/`. Multipliers are *estimates*
(verifier-adjusted), not measurements; each item is retained only when its own paired
measurement holds.

Findings:
- Grounded no-JIT items chain from geomean 0.656 to ~0.74 (band 0.72-0.78) of Bun no-JIT.
  1.2x still needs a ~1.6x uniform uplift, concentrated in the parser/string cluster.
- `--jit` has never been measured at the current head; the ~6% figure comes from an older
  branch. M0 (measure, then make `--jit` a superset of the lane) comes first.

Measured profiles behind the session's choices (callgrind, `target-prof` build; marginal =
N-iteration run minus a 2-iteration run):
- doxbee-promise: 905 M instructions per iteration; promise/job machinery 29%, lane 22%,
  GC 20% (about 50 M instructions per collection, whole-heap traces; finding on TOD-164).
- typescript-lib: 107 G instructions per iteration; lane 55%. Field cache sites overflow
  the u16 site space: 10.5 M+ lookups per iteration land on the shared uncached site
  (babel-wtb overflows too, but its overflow sites are cold).
