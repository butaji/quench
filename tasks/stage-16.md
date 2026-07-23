# Stage 16 — test/language/statements/class

**Status:** in_progress · **Path:** `test/language/statements/class` ·
**4,367 tests** · **4362 pass / 5 fail (99.9%)** as of 2026-07-23.

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
| 2026-07-23 | **4152** | **215** | **95.1%** | Destructuring private assign via assign_to; object rest LHS; nested outer private fields |
| 2026-07-23 | **4181** | **186** | **95.7%** | Computed destructuring param keys; private getter/setter pairs; super private dedup; async arrow is_async |
| 2026-07-23 | **4192** | **175** | **96.0%** | Derived explicit return; IsConstructor proxy/bound/async; yield* generators; static private method assign |
| 2026-07-23 | **4207** | **160** | **96.3%** | Async return TCO fix; static constructor naming; NativeFunction super() |
| 2026-07-23 | **4211** | **156** | **96.4%** | Lexical this-binding for super() in finally; try/catch CF restore |
| 2026-07-23 | **4241** | **126** | **97.1%** | Private eval (scoped names, ctor env, static brand); yield-spread; gen args; Array length; class bind |
| 2026-07-23 | **4248** | **119** | **97.3%** | Proxy get traps; ctor ControlFlow::Return leak; private fields on proxy target; __lookupGetter__ |
| 2026-07-23 | **4259** | **108** | **97.5%** | hasParameterExpressions body env; setter Param defaults; eval undeclared-private SyntaxError |
| 2026-07-23 | **4266** | **101** | **97.7%** | Generator method param/body env; direct eval AllPrivateNamesValid always |
| 2026-07-23 | **4274** | **93** | **97.9%** | Const binding, private primitive get/put, super field target, arguments-callee, extends TDZ, Proxy/Symbol extends |
| 2026-07-23 | **4278** | **89** | **98.0%** | Super in static blocks, super field init in call_super_constructor, super without extends, assignment lowering |
| 2026-07-23 | **4282** | **85** | **98.1%** | Date/Number/ArrayBuffer builtin subclass auto-super, nested private field on parameter, eval_super_call NativeConstructor wrapper |
| 2026-07-23 | **4286** | **81** | **98.1%** | super.prop in static field arrow assigns to class; minimal DataView builtin + subclass |
| 2026-07-23 | **4298** | **69** | **98.4%** | finish_constructor on function super; AggregateError/SAB/WeakRef/BigInt64/Uint64; object literal spread + yield-spread-obj |
| 2026-07-23 | **4300** | **67** | **98.5%** | Object destructure param ToObject for string primitives (static-init-arguments) |
| 2026-07-23 | **4311** | **56** | **98.7%** | fn-name SetFunctionName (method/accessor); static name/length shadow; private static `#method` name; getOwnPropertyNames |
| 2026-07-23 | **4318** | **49** | **98.9%** | Generator completion undefined unless return; class field SetFunctionName; arrow no spurious empty `.name` |
| 2026-07-23 | **4329** | **38** | **99.1%** | Nested static private on Class; derived invalid return TypeError; static private setter brand |
| 2026-07-23 | **4331** | **36** | **99.2%** | Nested direct eval `arguments` in deferred class-field arrows |
| 2026-07-23 | **4332** | **35** | **99.2%** | Optional chain prefix before private field (`o?.c.#f`) |
| 2026-07-23 | **4338** | **29** | **99.3%** | Builtin subclass auto-super; field key cache + intercalated ordering; sparse numeric keys; WeakMap/WeakSet subclass |
| 2026-07-23 | **4346** | **21** | **99.5%** | For-of IteratorClose; static block/field sequencing; String subclass trim/length; frozen field TypeError |
| 2026-07-23 | **4351** | **16** | **99.6%** | Proxy field defineProperty traps; super.x in ctor; RegExp lastIndex; TCO skip native callees; this-before-super ReferenceError |
| 2026-07-23 | **4355** | **12** | **99.7%** | GeneratorFunction ctor (function* parse, empty prototype); extends-null constructorParent; class expr binding scope; symbol field storage; inside_super_call guard |
| 2026-07-23 | **4362** | **5** | **99.9%** | SuperProperty GetThisBinding order; destructuring target before getter; ToPrimitive→Symbol property keys; Uint8 indexed coercion; verifyProperty Symbol hasOwn |

## Top remaining clusters (~12)

| ~Count | Cluster | Fix direction |
|-------:|---------|---------------|
| ~3 | this-before-super / private field order | Refine ReferenceError vs TypeError; super-in-super-args |
| ~9 | Single-test edge cases | See `tasks/failures-16.json` |

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
