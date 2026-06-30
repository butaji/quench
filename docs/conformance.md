# TypeScript Conformance Harness

This document describes the TypeScript conformance test harness in `crates/quench-runtime/tests/conformance.rs`.

## Overview

The harness runs TypeScript test cases from the `tests/typescript/` submodule against quench-runtime, measuring how many pass, fail, or are skipped. It is used to track runtime correctness and identify missing features.

## Run Modes

The harness supports three execution modes:

| Mode | Description | Use case |
|------|-------------|----------|
| `BaselineJs` | Run the pre-compiled `.js` baseline extracted from TypeScript output | Reference comparison |
| `SourceTs` | Run the original `.ts`/`.tsx` source directly via `ctx.eval_ts()` | Measures TypeScript-stripping quality |
| `Hybrid` | Try source-direct first; fall back to baseline on parse errors | Best-of-both for whitelist |

The **source-direct mode** is the primary measurement. It tells us which TypeScript features the runtime can execute directly from source, and which need a TypeScript compile step.

## Architecture

```
tests/typescript/
  tests/cases/conformance/   ← test corpus
  tests/baselines/reference/  ← TypeScript-emitted .js files

crates/quench-runtime/tests/
  conformance.rs              ← harness implementation
```

### Key components

**`TestCase`** — parsed from a `.ts` file:
- `ts_source`: original TypeScript source
- `baseline_js`: extracted `.js` from baselines directory (if available)
- `skip_reason`: reason to skip (None = run it)

**`TestResult`** — outcome of a test run:
- `Passed` / `Failed(String)` / `Skipped(String)`
- `OutputMismatch` / `ErrorMismatch` (for output comparison)

**`RunMode`** — which code to execute:
- `BaselineJs`: the `.js` baseline
- `SourceTs`: the original `.ts` source directly
- `Hybrid`: source-direct with baseline fallback

**`ConformanceReport`** (Task 42) — machine-readable JSON report written to `target/conformance_report.json`:
```json
{
  "timestamp": "2026-06-30T23:00:00Z",
  "total": 200,
  "passed": 112,
  "failed": 46,
  "skipped": 42,
  "pass_rate": 70.9,
  "cases": [
    { "name": "es6/for-of1.ts", "category": "es6", "status": "pass", "message": null },
    { "name": "classes/classDecl.ts", "category": "classes", "status": "fail", "message": "..." }
  ]
}
```

## Skip rules (Task 41)

Cases are skipped when:
- `@noEmit: true` — TypeScript emits no JS
- `@emitDeclarationOnly: true` — only .d.ts output
- `@module: amd|umd|system|node16|nodenext|none` — unsupported module system
- `@jsx: react` — JSX react runtime not available
- In a skipped directory: `types/`, `interfaces/`, `symbols/`, `namespaces/`, etc.
- `.errors.txt` suffix — diagnostic-only test
- Type-only `declare var` in class context — TypeScript strips the declaration

## Multi-file cases (Task 38)

TypeScript test cases can contain `// @filename:` markers that split a single `.ts` file into multiple logical files. The `split_units()` function parses these markers and returns each unit as a `(filename, code)` pair.

```ts
// @filename: a.ts
export const x = 1;

// @filename: b.ts
import { x } from "./a";
console.log(x);
```

## Configuration-specific baselines (Task 39)

TypeScript test cases use directives that affect emitted JS:
```
// @target: es2015
// @module: commonjs
// @jsx: preserve
```

The harness tries to match the most specific baseline file first (e.g., `test.es2015.commonjs.js`) and falls back to simpler names.

## TypeScript emit helpers (Task 40)

TypeScript injects helper functions into emitted JS. The harness preloads minimal versions of these so baselines don't `ReferenceError`:

- `__extends` — prototype-chain setup for class inheritance
- `__assign` — `Object.assign` polyfill
- `__awaiter` — async function state machine
- `__decorate` — class decorator helper
- `__importStar` / `__importDefault` — CommonJS interop

## Running tests

### Quick sanity check
```bash
cargo test -p quench-runtime --test conformance test_source_direct_simple
```

### 50-case whitelist (fast)
```bash
cargo test -p quench-runtime --test conformance test_whitelist_source_direct -- --ignored
```

### Full whitelist (slow — ~15 min)
```bash
cargo test -p quench-runtime --test conformance test_full_whitelist_conformance -- --ignored
```

### All conformance tests (including non-whitelist)
```bash
cargo test -p quench-runtime --test conformance test_run_conformance -- --ignored
```

### With output
```bash
cargo test -p quench-runtime --test conformance test_whitelist_source_direct -- --ignored --nocapture
```

## Interpreting results

```
=== SOURCE-DIRECT test results ===
Total cases: 200
Passed (source direct): 112
Failed (parse error - unsupported TS syntax): 46
Failed (runtime error - JS semantics issue): 0
Skipped: 42
Pass rate: 70.9%
```

- **Passed (source direct)**: The runtime executed the TypeScript source directly. These are the cases where TypeScript stripping + JS execution work together.
- **Failed (parse error)**: Unsupported TypeScript syntax. These identify features the lowerer needs to handle.
- **Failed (runtime error)**: Supported TypeScript that doesn't execute correctly. These identify bugs in the interpreter.
- **Skipped**: Cases that can't produce meaningful runtime results (see Skip rules).

The JSON report at `target/conformance_report.json` has per-case details.

## Updating the whitelist

The whitelist is defined in `WHITELIST_DIRS` in `conformance.rs`:

```rust
static ref WHITELIST_DIRS: Vec<&'static str> = vec![
    "es5", "es6", "es7", "es2016", "es2017", "es2018", "es2019",
    "expressions", "statements", "functions", "classes", "enums",
    "constEnums", "async", "asyncGenerators", "generators", ...
];
```

To add a new category:
1. Add the directory name to `WHITELIST_DIRS`
2. Run `cargo test -p quench-runtime --test conformance test_whitelist_source_direct -- --ignored` to see how many cases it adds
3. Run the full whitelist to verify no regressions

To remove a broken case from consideration:
1. Find the failure reason
2. If it's a TypeScript-only feature, add a skip rule in `should_skip()`
3. If it's a runtime bug, fix the runtime and re-run

## Local pass-rate gate (Task 36)

The conformance harness can enforce a minimum pass rate locally via the `MIN_PASS_RATE` environment variable. There is no external CI; the gate is run on the developer's machine or in any local test runner.

```bash
MIN_PASS_RATE=0.50 cargo test -p quench-runtime --test conformance -- --nocapture
```

If the pass rate drops below the threshold, the test fails.

## Common issues

### "ReferenceError: X is not defined" in baseline JS
The baseline uses a TypeScript emit helper (`__extends`, `__awaiter`, etc.) that quench-runtime doesn't define. Fix: add the helper to `EMIT_HELPERS` in `conformance.rs`.

### "parse error" for valid TypeScript
The lowerer doesn't yet handle this construct. Check the error message for keywords like `interface`, `declare`, `namespace`, `as`, `keyof`, etc. These indicate the TypeScript-stripping pass needs updating.

### Baseline file not found
TypeScript baselines live in `tests/typescript/tests/baselines/reference/`. If a baseline is missing, either:
- The case uses `@noEmit` and produces no baseline
- The TypeScript submodule is not initialized (`git submodule update --init`)

### Test times out
The test has a 30-second per-case timeout. If a case hangs, it may indicate an infinite loop in the interpreter. Add `@timeout` directive to the case source to increase the limit, or fix the interpreter.

## Architecture diagram

```
.ts source
    │
    ▼
TestCase::from_path()
    │
    ├─► parse_directives() → should_skip() → skip or run
    │
    └─► get_js_code(RunMode)
            │
            ├─► SourceTs: ts_source ──────► ctx.eval_ts() ──► TestResult
            │
            ├─► BaselineJs: baseline_js ──► prepend(EMIT_HELPERS) ──► ctx.eval_ts() ──► TestResult
            │
            └─► Hybrid: try SourceTs, fall back to BaselineJs
                       │
                       ▼
                  ConformanceReport
                  (target/conformance_report.json)
```
