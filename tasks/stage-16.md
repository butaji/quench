# Stage 16 — test/language/statements/class

**Status:** done · **Path:** `test/language/statements/class`

```bash
# Full digest (parallel; writes tasks/failures-16.json with TEST262_JSON=1)
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture

# Fast verify after a fix
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_FAILED_JSON=tasks/failures-16.json \
  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

On 100% the runner prints `ALL STAGES COMPLETE`; that line is the gate
to advance to stage 17.

## Key fixes that unblocked this stage

- **R4** — delete dead TComp ✓
- **R5** — object-model spec bugs (symbol identity, keys, strict writes) ✓
- **Derived constructor / `super`** — `has_explicit_constructor` so empty
  `constructor() {}` does not auto-call `super`; uninitialized `this` →
  ReferenceError.
- **Yield-in-class computed keys** — `generator_replay.rs` suspends
  mid-class-eval, replays completed yields on resume.
- **Private eval** — scoped names, ctor env, static brand.
- **Subclass auto-super** — Date/Number/ArrayBuffer/TypedArray builtins.
- **SetFunctionName** — method/accessor naming, static name/length shadow.
- **Proxy field defineProperty traps**, **for-of IteratorClose**,
  **frozen field TypeError**, **optional chain prefix before private field**.

## How this stage was cleared

Follow Phase A in `tasks/10-ways-to-speed-up.md` / `tasks/refactor-plan.md`:

1. R4 → R5 → S2 digest → fix clusters with one reproducer `#[test]` per
   cluster next to `src/eval/class*`.
2. Grow R1 only for ops the clusters touch. Do **not** start full R0 here.

Harness tooling: `tasks/harness-roadmap.md`.

Do not edit `tests/test262.rs` or anything under `tests/test262/`.
