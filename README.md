# Quench

JavaScript runtime targeting **100% test262 ECMAScript conformance**, staged
to 100% per stage, implemented with the **minimum possible LOC** as a
**small Rust core** plus a **self-hosted JS builtins layer**. Native
TypeScript/TSX/JSX via OXC parsing + a custom tree-walking interpreter.

The Rust core is the smallest set the builtins cannot be written without:
parser, AST lowering, evaluator, value model, lexical environments,
context/realm, and a handful of crate-backed primitives (`regress` for
RegExp, `chrono` for Date math, `num-bigint` for BigInt, `serde_json`
for JSON, `urlencoding` for URI, `oxc` for parsing). Every pure spec
algorithm on top of those — `Array.prototype.*`, `String.prototype.*`,
`Object.*`, `Reflect.*`, `Error.*`, `Map/Set/WeakMap/WeakSet`,
`Promise.prototype.*`, the intrinsic iterator prototypes, etc. — is
authored in JS under `crates/quench-runtime/builtins/*.js` and bound to a
frozen `%ops%` object exposed from `eval/ops.rs`. JS is ~1/3 the LOC of
the equivalent Rust, so this split keeps the implementation dead simple
and spec-faithful. See `docs/architecture.md`.

## Quick Start

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Architecture

```
crates/quench-runtime/
├── src/
│   ├── parser.rs        # OXC → internal AST (TS/TSX/JSX)
│   ├── lower/          # AST lowering
│   ├── ast.rs          # internal AST
│   ├── interpreter.rs  # eval entry points
│   ├── eval/
│   │   └── ops.rs      # canonical spec abstract operations + %ops% bridge
│   ├── env.rs          # lexical environments
│   ├── value/          # Value, Object, Function, NativeFunction, JsError
│   ├── context/        # Context, Realm (intrinsic prototypes owned here)
│   ├── builtins/
│   │   ├── core/       # crate-backed primitives only
│   │   │   ├── regex.rs   # regress
│   │   │   ├── date.rs    # chrono
│   │   │   ├── bigint.rs  # num-bigint
│   │   │   ├── json.rs    # serde_json
│   │   │   └── uri.rs     # urlencoding
│   │   ├── bootstrap.rs # parses + evaluates builtins/*.js at realm init
│   │   └── mod.rs
│   └── test262/        # staged runner (no checkpoints, no skips)
└── builtins/           # self-hosted JS builtins, embedded via include_str!
    ├── _intrinsics.js  # %ops% destructure (resolved at parse time)
    ├── Object.js, Function.js, Error.js, Symbol.js,
    ├── Number.js, Boolean.js, String.js, Math.js,
    ├── Array.js, Iterator.js,
    ├── Map.js, Set.js, WeakMap.js, WeakSet.js,
    ├── Promise.js, JSON.js, Reflect.js, Proxy.js,
    ├── RegExp.js, Date.js, BigInt.js,
    ├── TypedArray.js, ArrayBuffer.js, DataView.js, Atomics.js,
    └── decodeURI.js, encodeURI.js
```

## test262 Runner — 93 stages, no skips, no checkpoints

**93 ordered stages** — harness → language (lex → types → statements → modules) → built-ins (globals → constructors → iterators → collections → advanced) → annexB.

The stage list lives in `crates/quench-runtime/src/test262/runner.rs::STAGES`
and mirrors `tasks/index.json` exactly. Each stage runs to **100%
passing** before the next stage is touched. No checkpoints. No skips.
Two intentional exclusions only — `test/intl402` (ECMA-402, separate
suite) and `test/staging` (pre-draft cases); nothing else is out of
scope.

```bash
# Run current stage (see tasks/index.json `current_stage`)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Run a specific stage
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Run every stage in order, stop on first failure
ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

On 100% a stage prints `ALL STAGES COMPLETE — Stage N: X/X`; CI's summary
job greps that line to flip a stage to ✅. With `ALL_STAGES=1` a clean
full run prints `ALL STAGES COMPLETE — 93 stages passed`.

Strict mode: every non-`raw` test runs sloppy, then with `"use strict";`. `raw`/`noStrict` run sloppy only, `onlyStrict` strict only.

## Minimum-LOC, staged to 100%

Two compounding goals:

1. **Every stage at 100%** — `tasks/index.json` enumerates them; no
   stage is "good enough" until its bar is green. Advancing past a
   stage means it stays green under every later change too.
2. **Minimum total LOC** — the metric is implementation lines across
   the Rust core *and* the JS builtins layer, not per-PR diffs.
   Strategies:
   - **Small Rust core.** The core is parser/lower/eval/value/env/
     context + a few crate-backed primitives in `builtins/core/`.
     Anything expressible as a spec algorithm on top of `%ops%` lives
     in JS. See `docs/architecture.md`.
   - **One canonical `%ops%`.** Every spec abstract op — `ToPrimitive`,
     `ToPropertyKey`, `IteratorNext`, `IteratorClose`,
     `CreateDataPropertyOrThrow`, `OrdinaryHasProperty`, `IsCallable`,
     `SameValueZero`, … — exists exactly once, in `eval/ops.rs`, and is
     exposed to JS as a frozen `%ops%` object. Re-implementing one
     anywhere else (Rust or JS) is forbidden.
   - **Prefer crates over hand-rolling.** Confirmed in `DEPENDENCIES.md`:
     `regress` (RegExp), `chrono` (Date), `num-bigint` (BigInt),
     `serde_json` (JSON), `urlencoding` (URI), `oxc` (parsing). A
     hand-rolled copy of any of these — including a thinly-disguised
     `chrono_*` helper that never imports `chrono` — is forbidden.
   - **No speculative generality.** Don't add slots, flags, hooks, enum
     variants, vtables, or storage maps that no current test262 stage
     exercises. If a refactor lands scaffolding with zero call sites,
     it's deleted in the same PR.
   - **Zero duplication.** `grep -R` before defining any symbol. Two
     structurally identical `fn`s in two files must be hoisted; an
     `enum` variant constructed nowhere is deleted.
   - **Dead code is a bug, not polish.** `pub fn` with zero callers
     outside its module is deleted. `#[allow(dead_code)]` is a
     `TODO(delete)` marker, not a permanent state.

The active cross-cutting cleanup queue is `tasks/refactor-plan.md`
(R0–R14), led by the self-hosting pivot (R0) and the canonical `%ops%`
home (R1).

## Workflow: unit tests, not guesswork — enforced, no exceptions

**You do not debug. You do not guess. You write a failing unit test first.
Every. Single. Time. No exceptions.**

A failing test262 case, a bug, a new builtin, a parser or evaluator change —
all enter the codebase through the same gate: a `#[test]` that asserts the
exact behavior, committed *before* any production change. If you cannot
express the behavior as a unit test, you do not understand it yet, and you are
not allowed to touch production code.

### Forbidden

- `println!` / `dbg!` archaeology. **Never.**
- Reading code until it "looks wrong" and patching. **Never.**
- "Let me try this" speculative edits. **Never.**
- Refactors done "while I'm here" without a test. **Never.**
- Skipping the failing-test step "just this once". **Never.**
- Editing `tests/test262.rs` or anything under `tests/test262/`. **Never.**

### Mandatory cycle, in order

1. **Reproduce** — add a `#[test]` in the module's `mod tests` (or
   `crates/quench-runtime/tests/`) that exercises the exact JS/Rust behavior.
   Mirror the style in `src/eval/string_methods.rs` and `src/builtins/map.rs`.
   For self-hosted JS builtins, the test lives in Rust and wraps the JS call
   through `Context::eval`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. If it doesn't fail, or fails for the wrong
   reason, delete the test and write a better one. Do not proceed.
3. **Fix** — minimal change to production code (Rust core, `builtins/*.js`,
   or `eval/ops.rs`) so the unit test passes. No opportunistic refactors.
4. **Verify** — re-run the unit test, the module's whole test suite, then the
   relevant test262 stage. `cargo fmt` / `cargo clippy --all-targets` until
   both are clean. Linter warnings block the fix from being "done".
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

test262 output signals *what* to test; the reproduction lives as a unit test
next to the code — the conformance run in `tests/test262.rs` is never edited.

## TypeScript / JSX

```rust
let mut ctx = Context::new()?;
ctx.eval_typescript(include_str!("src/main.ts"))?;
```

See `crates/quench-runtime/tests/native_extensions.rs` for examples.

## CI

fmt → clippy → build → integration tests → test262 stages in parallel.

## License

MIT
