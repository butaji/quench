# Task 28: Implement TypeScript conformance harness core

## Goal

Build the reusable Rust components that the conformance runner needs to read TypeScript test cases, interpret directives, extract expected JS, and execute the JS in `quench-runtime`.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: support the directives and file shapes needed by the whitelist first; skip exotic harness features.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance/` (new module directory for harness code)
- `crates/quench-runtime/tests/conformance.rs` (test entry point)
- `crates/quench-runtime/Cargo.toml`

## Components to implement

### 1. `TestCase` struct

```rust
struct TestCase {
    path: PathBuf,
    name: String,
    directives: HashMap<String, Vec<String>>,
    files: Vec<VirtualFile>,
}

struct VirtualFile {
    name: String,
    source: String,
}
```

### 2. Directive parser

Parse `// @name: value` lines (case-insensitive names, comma-separated values). Store all values as `Vec<String>`.

Important directives to parse:

- `target` — default `es2015`
- `module` — default `commonjs`
- `jsx`, `jsxFactory`, `jsxImportSource`
- `lib`
- `strict`, `strictNullChecks`, `noImplicitAny`, etc.
- `noEmit`, `noEmitOnError`, `emitDeclarationOnly`
- `outFile`, `outDir`
- `downlevelIteration`
- `experimentalDecorators`, `emitDecoratorMetadata`
- `importHelpers`, `esModuleInterop`
- `sourceMap`, `inlineSourceMap`

### 3. Multi-file case splitter

If the source contains `// @filename: foo.ts` markers, split into multiple `VirtualFile`s. The first file has the test case's basename.

### 4. Baseline resolver

Given a case path and a chosen configuration, compute the baseline filename:

```text
<basename>.js
<basename>(target=es2015).js
<basename>(module=commonjs,target=es5).js
```

Sort directive keys deterministically when building the suffix so lookups are stable.

### 5. Baseline JS extractor

A `.js` baseline contains sections separated by headers like:

```js
//// [tests/cases/conformance/es6/arrowFunction/emitArrowFunctionES6.ts] ////

//// [emitArrowFunctionES6.js]
"use strict";
var f1 = () => { };
```

The extractor must:

- Split on `//// [`.
- Keep only sections whose file name ends with `.js`.
- Strip the trailing `] ////` marker.
- Normalize CRLF → LF.
- Concatenate the emitted JS sections in file order.

### 6. Skip / filter rules

A case is skipped if:

- The resolved baseline does not exist.
- A non-empty `.errors.txt` baseline exists for the chosen configuration.
- `noEmit: true` or `emitDeclarationOnly: true`.
- The module system is unsupported (`amd`, `umd`, `system`, `node16`, `nodenext`).
- JSX is used and no JSX runtime stub is registered.
- Decorators/metadata are used and no helper stubs are registered.
- The directory is type-check-only.

### 7. Runner integration

For each non-skipped case:

1. Concatenate the virtual source files (if multi-file) or use the single source.
2. Parse with `swc_ecma_parser` TypeScript syntax.
3. Lower to the runtime HIR (stripping types).
4. Evaluate the HIR in a fresh `quench_runtime::Context`.
5. Capture runtime errors.
6. Optionally compare console output against a baseline (not required for first version).

## Steps

1. Add dev-dependencies (`walkdir`, `regex` or `lazy_static`) if needed.
2. Create `crates/quench-runtime/tests/conformance/mod.rs` and submodules:
   - `parser.rs` — directive parser and multi-file splitter
   - `baseline.rs` — baseline resolver and JS extractor
   - `filter.rs` — skip rules
   - `runner.rs` — runner integration
3. Write unit tests for each component before implementation.
4. Expose a `run_conformance(categories: &[&str]) -> Report` function from `conformance.rs`.

## Boundaries

- Only add test harness code in `crates/quench-runtime/tests/`.
- Do not modify `tests/typescript/`.
- Do not change runtime behavior in this task; the runner may report failures.

## Acceptance criteria

- `cargo test -p quench-runtime --test conformance` compiles and runs.
- The harness can parse directives, split multi-file cases, extract JS baselines, and skip cases correctly.
- At least one simple conformance case runs end-to-end in `quench-runtime`.

## Verification

```bash
cargo test -p quench-runtime --test conformance
```
