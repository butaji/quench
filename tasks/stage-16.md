# Stage 16 — test/language/statements/class

**Status:** in_progress · **Path:** `test/language/statements/class` ·
stages 00–15 done · **4,367 tests** · **29 stages / 2,154 tests done (4%)**.

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
