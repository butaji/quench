# Plan — shortest path to 100% test262

Goal: 100% of test262 (no skips, stages gate in `tasks/index.json`
order), ASAP, with minimum LOC. Architecture: **OXC + walker**
(`docs/architecture.md`).

Live data: `tasks/index.json` (stage status/counts, `current_stage`),
`tasks/failures-N.json` (digest clusters). **Never duplicate those
numbers here** — measured counts, rates, and occurrence stats rot
with a single code change; this file holds decisions and reasoning
only.

## Why this order is shortest

Measured evidence, not preference (re-verify against the current
digest before acting on any of these):

- Language failures are dominated by **early errors** (static
  semantics). Example: the `for-of` stage's dominant cluster is
  parse-negative tests the parser must reject (`for (var x of [])
  function f() {}`). Hand-rolling early errors in `lower/` is
  thousands of LOC; `oxc_semantic` already implements them →
  **do it first**.
- `language/expressions` is nearly done in Rust → language stages
  need targeted fixes, not rewrites.
- Built-ins are the bottleneck by an order of magnitude (see
  `index.json`: the overwhelming majority of recorded failures).
  Builtins in JS are ~1/3 the LOC of Rust and ride on one `__ops__`
  path → **self-host before grinding built-ins**; every Rust builtin
  line written now is deleted later.
- `Temporal` is solved by `temporal_rs` (Boa/V8-proven) with zero
  coupling to the interpreter core and is the largest single failure
  block → best tests-per-LOC in the plan; **start in parallel with
  Phase B, not last**.

## Critical path

```
Phase A — language stages (from current_stage onward)
  A0. Upgrade oxc → latest, fix parser.rs/lower/ breakage
      (mandatory — DEPENDENCIES.md OXC version policy)
  A1. oxc_semantic early errors        ← top lever, unblocks every
      (parse → oxc_semantic → SyntaxError before lowering;
       same version as oxc; DEPENDENCIES.md row in same diff)
  A2. Fix remaining stages by digest cluster, in index order
      (one reproducer #[test] per dominant cluster → fix → re-digest)
  A3. url crate replaces urlencoding   (before the modules stage)

Phase B — gate before built-ins (the Object stage)
  B0. Temporal via temporal_rs + zoneinfo_rs — runs in parallel
      with B; largest single failure block, zero core coupling
  B1. Realm owns all intrinsics        ← the object-model endgame:
      kills thread-local proto caches AND the IntrinsicSnapshot
      bridge; $262.createRealm/bootstrap become trivial; required
      later by ShadowRealm anyway
  B2. Finish __ops__: eval/ops.rs owns implementations (no re-exports)
  B3. Self-host builtins in JS on __ops__, Object first, then the
      dependency order in docs/architecture.md
  B4. One iterator protocol (%IteratorPrototype%); chrono Date core

Phase C — built-ins → annexB → done
  C1. Author built-ins in JS only — never expand Rust builtins
  C2. Async: VERIFY FIRST — the async-function/async-generator
      stages are nearly passing, so the async→generator lowering may
      be largely built; budget the remainder, not a fresh transform
  C3. RegExp Unicode \p{} via regex crate (before the RegExp stage)
  C4. Proxy hybrid: Rust traps + JS invariant checks
  C5. annexB — browser-compat semantics; digest-driven, mostly small
      per-builtin quirks
```

Rules: stages stay a sequential gate; within a stage, fix by cluster
frequency. **A cluster spanning multiple pending stages gets ONE
root-cause fix**, then re-run every affected stage (example shape:
`*-fn-name-*` inference clusters recur across destructuring-heavy
stages — fixing them per-stage repeats the same work).
`tasks/index.json` is the only live truth; `failures-N.json` are
working artifacts — regenerate at the start of each stage, never
trust a stale digest. Lint gate applies to every touched file
(`.clippy.toml` limits, zero warnings). Every fix enters via a
failing `#[test]` (AGENTS.md).

Simplification refactors (`tasks/refactor-simplicity.md`): R0–R2 ride
inline during stage fixes (mechanical, quick). R3 (Completion type)
is a LOC/clarity lever, NOT a conformance lever — digests show
failure clusters are early-errors/destructuring/TDZ, not completion
plumbing — so it lands at the A→B boundary, strictly before B2/B3 so
__ops__ and the JS builtins aren't re-churned. R4 after B1.

## LOC budget (remaining, rough sizing)

Estimates for capacity planning, not measurements — update only when
a phase boundary is crossed, never per-fix.

| Work | Rust | JS |
|---|---|---|
| Phase A (early errors + language fixes) | ~3,000 | 0 |
| Phase B (Realm intrinsics, __ops__, bootstrap) | ~800 | ~1,000 |
| Temporal (temporal_rs wrapper, B0) | ~500 | ~600 |
| Built-ins in JS (Object→Atomics) | ~1,500 | ~12,000 |
| Async (remainder) / RegExp / Proxy / buffers | ~1,200 | ~1,000 |
| annexB quirks | ~200 | ~300 |
| **Total** | **~7,200** | **~14,900** |

Reference points: Boa reaches ~94% with roughly 25k lines of Rust;
QuickJS ~83% with roughly 80k lines of C.

## Rejected (do not resurrect)

- Full self-host pivot before language stages finish (delays the gate;
  payoff starts at built-ins).
- Grinding Object/Array/String in Rust (dies under B3).
- Bytecode/JIT (test262 tests behavior, not speed; large LOC cost).
- swc for async transforms; tokio for the microtask queue.
- fancy-regex/re2 (no `\p{}`); wasmtime for ShadowRealm.
- Skip lists / per-stage checkpoints — the gate now fails on skips.
- NaN-boxing / bumpalo / string interning before 100% — performance
  work is post-conformance (test loop speed is not the bottleneck).
