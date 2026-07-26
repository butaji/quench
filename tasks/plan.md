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
- `built-ins/Object` digest: 1,783/3,411 failing. The built-ins half
  (~28k tests) is the bottleneck. Builtins in JS are ~1/3 the LOC of
  Rust and ride on one `%ops%` path → **self-host before grinding
  built-ins**; every Rust builtin line written now is deleted later.
- `Temporal` (4,603 tests) is solved by `temporal_rs` (Boa/V8-proven)
  → last, cheap.

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
  B1. Realm owns all intrinsics        ← the object-model endgame:
      kills thread-local proto caches AND the IntrinsicSnapshot
      bridge; $262.createRealm/bootstrap become trivial; required
      later by ShadowRealm (118) anyway
  B2. Finish %ops%: eval/ops.rs owns implementations (no re-exports)
  B3. Self-host builtins in JS on %ops%, Object first, then the
      dependency order in docs/architecture.md
  B4. One iterator protocol (%IteratorPrototype%); chrono Date core

Phase C — built-ins → annexB → Temporal (stage 71 → 122)
  C1. Author built-ins in JS only — never expand Rust builtins
  C2. Async: hand-rolled async→generator at lower time (~500 LOC).
      Boa reaches 94% this way; swc stack rejected (10+ crates,
      second parser)
  C3. RegExp Unicode \p{} via regex crate (before stage 84)
  C4. Proxy hybrid: Rust traps + JS invariant checks (stage 115)
  C5. Temporal last: temporal_rs + zoneinfo_rs (stage 120)
```

Rules: stages stay a sequential gate; within a stage, fix by cluster
frequency. Lint gate applies to every touched file (≤500 lines/file,
≤40 lines/fn, complexity ≤10, zero clippy warnings). Every fix enters
via a failing `#[test]` (AGENTS.md).

## LOC budget (remaining, approximate)

| Work | Rust | JS |
|---|---|---|
| Phase A (early errors + language fixes) | ~3,000 | 0 |
| Phase B (Realm intrinsics, %ops%, bootstrap) | ~800 | ~1,000 |
| Built-ins in JS (Object→Atomics) | ~1,500 | ~12,000 |
| Async / RegExp / Proxy / buffers | ~1,500 | ~1,000 |
| Temporal (temporal_rs wrapper) | ~500 | ~600 |
| **Total** | **~7,500** | **~15,000** |

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
