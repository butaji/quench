# 31 — Bounded-polymorphic guard caches (2-4 shapes/targets)

Status: complete

Evidence from the V8v7 suite (see benchmark survey referenced in this task) shows that richards' `tcb.task.run(packet)` cycles a single call site through 4 concrete task constructors (`IdleTask`/`DeviceTask`/`WorkerTask`/`HandlerTask`) every scheduler iteration, and deltablue's `Planner.addPropagate`/`Plan.execute` dispatch across 5+ concrete `Constraint` subtypes at shared call/property sites. Neither is monomorphic, and neither is unboundedly megamorphic — both cycle a small, fixed, closed set. A guard family (from [[25-generalized-speculative-guards]]) that only caches one shape/target will thrash on every iteration for these two benchmarks, converting what should be a fast guarded path into permanent generic-path execution.

Extend the `ShapeGuarded<S>` / `CallTargetGuarded<F>` connectors to a small closed cache (2-4 entries) checked in sequence before falling back to the generic path. This stays a coproduct choice over a bounded set of typed morphisms — no change to the composition-typechecking mechanism itself, only to how many guarded candidates a site may hold simultaneously.

Acceptance: richards' scheduler call site and deltablue's constraint dispatch sites hit a cached guard on every iteration after warmup (verified by a counter distinguishing cache-hit-slot-N from generic-path fallback); a truly megamorphic site (more distinct shapes than the cache size) degrades to the generic path rather than evicting and re-populating every call; cache size is a fixed small constant, not per-site unbounded growth.

Current phase: own-property polymorphism has been measured and rejected below. The
remaining bounded-polymorphic work is call-target guards; prototype-chain caches are
tracked separately by [[08-property-inline-caches]].

## Property-PIC result: rejected

Two fixed four-way own-property representations were implemented, tested, measured,
and reverted:

- A single `Cell<[Option<Entry>; 4]>` copied/scanned the whole coproduct even on the
  common monomorphic hit. Focused six-run medians in
  `reports/task31-property-pic-focused-ab-6/comparison.txt` regressed Richards 6.63%
  and DeltaBlue 6.30%.
- A primary hot cell plus three cold overflow cells preserved the original first-arm
  check. Its diagnostic counters showed Richards 1,271,854 primary hits and **zero**
  overflow hits; DeltaBlue had 835,189 primary and only 31,059 second-arm hits. Neither
  saturated, while misses remained 243,605 and 538,017 respectively. Evidence is in
  `reports/task31-richards-property-pic-stats.txt` and
  `reports/task31-deltablue-property-pic-stats.txt`.

The refined implementation's complete six-run gate in
`reports/task31-primary-property-pic-full-ab-6/comparison.txt` measured aggregate
−1.57% and Navier–Stokes −6.21%, so it was also reverted. The accepted Task 123 binary
and source hash are restored exactly. The evidence disproves bounded own-shape
polymorphism as the next broad win: continue this task only for call-target arms.
Prototype/absent-property misses belong to [[08-property-inline-caches]], where a
prototype-chain guard can address the dominant miss population without penalizing
monomorphic own-property hits.

## Four-arm call-target result: rejected

A fixed four-arm `OnceCell` coproduct was allocated only for actual `Call` bytecodes.
Each arm retained the callable identity and its already-linked dynamic-code/environment
or native-function target. Hits called the same canonical dynamic/native helpers as the
generic dispatcher; a fifth identity saturated the site without eviction. A focused
test proves all four identities remain valid and the fifth stays on fallback. All 63
release tests and complete semantic smoke passed.

Diagnostics demonstrate real polymorphism and correct bounded behavior. Richards
recorded 3,332,299 / 43,055 / 255,483 / 91,999 hits by arm, only 73 misses, and no
saturation. DeltaBlue recorded 4,504,981 / 596,083 / 118,577 / 0 hits, 206,082 misses,
and 11 saturated sites. Evidence is in `reports/task31-richards-call-pic-stats.txt` and
`reports/task31-deltablue-call-pic-stats.txt`.

Despite that coverage, the focused comparison in
`reports/task31-call-pic-focused-ab-6/comparison.txt` improved only 0.93%, and the exact
full-suite comparison in `reports/task31-call-pic-full-ab-4/comparison.txt` regressed
aggregate 1.14% (1182.38 to 1168.88). The four-way scan and retained-target footprint
cost more than generic dispatch. The bounded implementation is rejected; [[27-callsite-devirtualization]]
isolated the evidence-backed first arm before the source was fully restored to
[[125-borrowed-callee-dispatch]]. Both bounded variants are therefore closed as measured
negative experiments rather than left as latent runtime machinery.
