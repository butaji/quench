# Plan — shortest path to 100% test262

Goal: 100% of test262 (all 50k+ tests, no skips, stages gate in
`tasks/index.json` order), ASAP, with minimum LOC. Architecture:
**OXC + walker** (`docs/architecture.md`).

Live data: `tasks/index.json` (stage status/counts, `current_stage`),
`tasks/failures-N.json` (digest clusters). Never duplicate those numbers
here.

## Why this order is shortest

Measured evidence, not preference:

- Language failures are dominated by **early errors** (static
  semantics). Example: stage 25 (`for-of`) cluster is parse-negative
  tests the parser must reject (`for (var x of []) function f() {}`).
  Hand-rolling early errors in `lower/` is thousands of LOC;
  `oxc_semantic` already implements them → **do it first**.
- `language/expressions` (11,101 tests) is nearly done in Rust
  (31 failures) → language stages need targeted fixes, not rewrites.
  Language overall: 1,046/23,711 failing (4.4%).
- Built-ins are the bottleneck by an order of magnitude:
  **16,706/23,668 failing (70.6%) — 89% of all recorded failures**
  (annexB: 1,086/1,086). Builtins in JS are ~1/3 the LOC of
  Rust and ride on one `%ops%` path → **self-host before grinding
  built-ins**; every Rust builtin line written now is deleted later.
- `Temporal` (4,603 tests at 0% — 24% of ALL recorded failures) is
  solved by `temporal_rs` (Boa/V8-proven) with zero coupling to the
  interpreter core → best tests-per-LOC in the plan; **start in
  parallel with Phase B, not last**.

## Critical path

```
Phase A — language stages (now, stage 25 → 56)
  A0. Upgrade oxc 0.47 → latest (0.141), fix parser.rs/lower/ breakage
      (mandatory — DEPENDENCIES.md OXC version policy)
  A1. oxc_semantic early errors        ← top lever, unblocks every
      (parse → oxc_semantic → SyntaxError before lowering;
       same version as oxc; DEPENDENCIES.md row in same diff)
  A2. Fix remaining stages by digest cluster, in index order
      (one reproducer #[test] per dominant cluster → fix → re-digest)
  A3. url crate replaces urlencoding   (before stage 53 modules)

Phase B — gate before built-ins (stage 71 Object)
  B0. Temporal via temporal_rs + zoneinfo_rs (stage 120) — runs in
      parallel with B; largest single failure block (4,603 tests)
  B1. Realm owns all intrinsics        ← the object-model endgame:
      kills thread-local proto caches AND the IntrinsicSnapshot
      bridge; $262.createRealm/bootstrap become trivial; required
      later by ShadowRealm (118) anyway
  B2. Finish %ops%: eval/ops.rs owns implementations (no re-exports)
  B3. Self-host builtins in JS on %ops%, Object first, then the
      dependency order in docs/architecture.md
  B4. One iterator protocol (%IteratorPrototype%); chrono Date core

Phase C — built-ins → annexB → done (stage 71 → 122)
  C1. Author built-ins in JS only — never expand Rust builtins
  C2. Async: VERIFY FIRST — stages 13/38 are ~99% passing, so the
      async→generator lowering may be largely built; budget below
      assumes the remainder, not a fresh ~500 LOC
  C3. RegExp Unicode \p{} via regex crate (before stage 84)
  C4. Proxy hybrid: Rust traps + JS invariant checks (stage 115)
  C5. annexB (stage 121, 1,086 tests, 0% passing) — browser-compat
      semantics; digest-driven, mostly small per-builtin quirks
```

Rules: stages stay a sequential gate; within a stage, fix by cluster
frequency. **A cluster spanning multiple pending stages gets ONE
root-cause fix**, then re-run every affected stage (observed:
`dstr/*-fn-name-*` inference fails in stages 30, 34, and others —
fixing it per-stage repeats the same work). `tasks/index.json` is
the only live truth; `failures-N.json` are working artifacts —
regenerate at the start of each stage, never trust a stale digest.
Lint gate applies to every touched file (≤500 lines/file,
≤40 lines/fn, complexity ≤10, zero clippy warnings). Every fix enters
via a failing `#[test]` (AGENTS.md).

Simplification refactors (`tasks/refactor-simplicity.md`): R0–R2 ride
inline during stage fixes (mechanical, ≤1 day each). R3 (Completion
type) is a LOC/clarity lever, NOT a conformance lever — digests show
failure clusters are early-errors/destructuring/TDZ, not completion
plumbing — so it lands at the A→B boundary, strictly before B2/B3 so
%ops% and the JS builtins aren't re-churned. R4 after B1.

## LOC budget (remaining, approximate)

| Work | Rust | JS |
|---|---|---|
| Phase A (early errors + language fixes) | ~3,000 | 0 |
| Phase B (Realm intrinsics, %ops%, bootstrap) | ~800 | ~1,000 |
| Temporal (temporal_rs wrapper, B0) | ~500 | ~600 |
| Built-ins in JS (Object→Atomics) | ~1,500 | ~12,000 |
| Async (remainder) / RegExp / Proxy / buffers | ~1,200 | ~1,000 |
| annexB quirks | ~200 | ~300 |
| **Total** | **~7,200** | **~14,900** |

Reference: Boa ~25k Rust → 94%; QuickJS ~80k C → 83%.

## Rejected (do not resurrect)

- Full self-host pivot before language stages finish (delays the gate;
  payoff starts at built-ins).
- Grinding Object/Array/String in Rust (dies under B3).
- Bytecode/JIT (test262 tests behavior, not speed; +5–10k LOC).
- swc for async transforms; tokio for the microtask queue.
- fancy-regex/re2 (no `\p{}`); wasmtime for ShadowRealm.
- Skip lists / per-stage checkpoints — the gate now fails on skips.
- NaN-boxing / bumpalo / string interning before 100% — performance
  work is post-conformance (test loop speed is not the bottleneck).
