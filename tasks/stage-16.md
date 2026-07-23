# Stage 16 — test/language/statements/class

**Status:** in_progress · **Path:** `test/language/statements/class` ·
**4,367 tests** · **4147 pass / 220 fail (95.0%)** as of 2026-07-23.

```bash
# Full digest (parallel; writes tasks/failures-16.json with TEST262_JSON=1)
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture

# Fast verify after a fix
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_FAILED_JSON=tasks/failures-16.json \
  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

On 100% the runner prints `ALL STAGES COMPLETE — Stage 16: N/N`; that
line is the gate to advance to stage 17.

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 start | 4080 | 287 | 93.4% | Iterator destructuring, private eval/brand |
| 2026-07-23 | 4110 | 257 | 94.1% | PatternDeclaration, default param TDZ |
| 2026-07-23 | **4119** | **248** | **94.3%** | Reflect.has, private method `.name`, Array subclass instanceof |
| 2026-07-23 | **4126** | **241** | **94.5%** | Error subclass super() preserves derived prototype |
| 2026-07-23 | **4145** | **222** | **94.9%** | Symbol computed field keys; Object/Promise/Function subclass instanceof |
| 2026-07-23 | **4147** | **220** | **95.0%** | for-of/for-in member+private LHS lowering (private field brand checks) |

## Top remaining clusters (~222)

| ~Count | Cluster | Fix direction |
|-------:|---------|---------------|
| 14 | Value is not a function, got undefined | Residual private ref/proxy |
| 17 | Expected Test262Error not thrown | Destructuring eval-order |
| 17 | Expected TypeError not thrown | Abrupt completion / brand |
| 11 | Private method or accessor already defined | Escape-sequence private names |
| 9 | Value is not iterable | yield* spread |
| 8 | Expected SyntaxError (privatename eval) | Direct eval early errors |
| 8 | Expected ReferenceError not thrown | TDZ / uninitialized this |
| ~20 | Missing builtins (DataView, TypedArrays, …) | Stage-gated primitives |

## How to clear this stage (ASAP × min LOC)

Follow Phase A in `tasks/10-ways-to-speed-up.md` / `tasks/refactor-plan.md`:

1. ~~**R4**~~ — delete dead TComp ✓
2. ~~**R5**~~ — object-model spec bugs (symbol identity, keys, strict writes) ✓
3. **S2 digest** — re-run full digest on all 4367 files (harness no longer
   skips subdirs silently). Group failures; one reproducer `#[test]` per
   cluster next to `src/eval/class*`.
4. **Derived constructor / `super`** — largest expected cluster (~40+):
   `has_explicit_constructor` so empty `constructor() {}` does not
   auto-call `super`; uninitialized `this` → ReferenceError. WIP in stash
   `wip-class`.
5. Grow **R1** only for ops the clusters touch. Do **not** start full R0 here.

Harness tooling: `tasks/harness-roadmap.md`.

Do not edit `tests/test262.rs` or anything under `tests/test262/`.

## Known failure clusters (pre-full digest; re-measure after harness land)

| Priority | Cluster | Fix direction |
|----------|---------|---------------|
| P0 | Derived ctor without `super` → ReferenceError | `has_explicit_constructor` + `finish_ctor_result` |
| P1 | Missing builtins (DataView, AggregateError, …) | Stage-gated; defer until built-in stages unless blocking class syntax |
| P1 | Subclass own props (`length`/`name`/`message`) | Error subclass semantics |
| P2 | `arguments.callee` in class bodies | Strict mode / arguments object |
| P2 | Stack overflow (10 crash files) | Fix recursion; remove path skips |

## History

- 2026-07-23 — R4 TComp deleted (~470 LOC); R5 symbol identity + object-model spec fixes landed on `main` lineage.
- 2026-07-23 — Harness S5: parallel digest, explicit skips, JSON + failed-only rerun (`tasks/harness-roadmap.md`).
- 2026-07-23 — Derived ctor fix (`has_explicit_constructor`): explicit `constructor() {}` without `super` → ReferenceError.
- 2026-07-23 — QUICK digest (913 files sampled): **666 pass / 247 fail / 0 skip**. Top cluster: `TypeError: Cannot read property 'prototype' of undefined` (~228, yield-in-class). Stack overflow: `dstr/async-private-gen-meth-*`, `prototype-wiring.js` (fix recursion, not skip).
- 2026-07-23 — **Yield-in-class computed keys** fixed: `generator_replay.rs` suspends mid-class-eval, replays completed yields on resume (`accessor-name-inst-computed-yield-expr.js` passes). Re-run full digest to measure cluster drop.
- 2026-07-23 — **Generator env persistence** + `return yield` handling: cpn-class-decl yield cluster (4 files) passes.
- 2026-07-23 — **Multi-level super()** fix in `call_super_constructor`: `set_super_class` + `set_this_value`; `prototype-wiring.js` passes (removed from crash skip list).
