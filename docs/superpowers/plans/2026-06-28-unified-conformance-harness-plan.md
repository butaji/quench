> **I'm using the writing-plans skill to create the implementation plan.**

# Unified Conformance Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run both `tests/test262` and `tests/typescript` conformance suites from a single Rust integration test. Replace the Node-based TypeScript runner. All harness helpers are Rust native functions.

**Architecture:** `crates/quench-runtime/src/conformance/` contains shared reporting, a test262 runner with native helpers and frontmatter parsing, and a TypeScript runner with directive/baseline parsing. The integration test `crates/quench-runtime/tests/conformance.rs` dispatches both suites and writes JSON reports.

**Tech Stack:** Rust, `swc_ecma_parser`, `swc_ecma_transforms_typescript`, `swc_ecma_transforms_react` (optional), `serde_yaml`, `walkdir`, `serde_json`.

---

## Task 1: Add TypeScript parser and `Context::eval_ts`

**Files:**
- Modify: `crates/quench-runtime/Cargo.toml`
- Modify: `crates/quench-runtime/src/swc_parse.rs`
- Modify: `crates/quench-runtime/src/lib.rs`

- [ ] **Step 1: Add dependencies**

In `crates/quench-runtime/Cargo.toml`:

```toml
swc_ecma_transforms_base = "0.143"
swc_ecma_transforms_typescript = "0.197"
swc_ecma_transforms_react = "0.197"
```

- [ ] **Step 2: Write the failing test**

Create `crates/quench-runtime/tests/eval_ts.rs`:

```rust
use quench_runtime::Context;

#[test]
fn eval_ts_strips_types() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_ts("const x: number = 1 + 2; x;");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn eval_ts_rejects_invalid_typescript() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_ts("const x: = 1;");
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cargo test -p quench-runtime --test eval_ts -- --nocapture
```

Expected: FAIL with "no method named `eval_ts`".

- [ ] **Step 4: Implement TypeScript parsing + strip**

In `crates/quench-runtime/src/swc_parse.rs` add:

```rust
use swc_ecma_parser::TsSyntax;
use swc_ecma_transforms_typescript::strip;
use swc_ecma_visit::FoldWith;
use swc_ecma_ast::Program as SwcProgram;

pub fn parse_ts(source: &str, is_module: bool) -> Result<Program, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );
    let syntax = Syntax::Typescript(TsSyntax {
        tsx: false,
        decorators: false,
        ..Default::default()
    });
    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let program = if is_module {
        SwcProgram::Module(parser.parse_module().map_err(|e| JsError(format!("Parse error: {:?}", e)))?)
    } else {
        SwcProgram::Script(parser.parse_script().map_err(|e| JsError(format!("Parse error: {:?}", e)))?)
    };
    let program = program.foldWith(&mut strip());
    match program {
        SwcProgram::Script(script) => lower_script(&script).map_err(|e| JsError(e.to_string())),
        SwcProgram::Module(module) => lower_module(&module).map_err(|e| JsError(e.to_string())),
    }
}
```

- [ ] **Step 5: Add `Context::eval_ts`**

In `crates/quench-runtime/src/lib.rs`:

```rust
pub fn eval_ts(&mut self, source: &str) -> Result<Value, JsError> {
    let program = crate::swc_parse::parse_ts(source, false)?;
    crate::interpreter::eval_program(&program, &mut self.env)
}

pub fn eval_ts_module(&mut self, source: &str) -> Result<Value, JsError> {
    let program = crate::swc_parse::parse_ts(source, true)?;
    crate::interpreter::eval_program(&program, &mut self.env)
}
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test -p quench-runtime --test eval_ts -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/quench-runtime/src/swc_parse.rs crates/quench-runtime/src/lib.rs crates/quench-runtime/tests/eval_ts.rs crates/quench-runtime/Cargo.toml
git commit -m "feat(runtime): add Context::eval_ts using swc strip"
```

---

## Task 2: Create shared conformance module and report types

**Files:**
- Create: `crates/quench-runtime/src/conformance/mod.rs`
- Create: `crates/quench-runtime/src/conformance/report.rs`

- [ ] **Step 1: Define shared outcome/report types**

```rust
// crates/quench-runtime/src/conformance/report.rs
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail { error: String },
    Skip { reason: String },
}

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub path: PathBuf,
    pub outcome: Outcome,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<CaseResult>,
}

impl Report {
    pub fn new(results: Vec<CaseResult>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|r| matches!(r.outcome, Outcome::Pass)).count();
        let failed = results.iter().filter(|r| matches!(r.outcome, Outcome::Fail { .. })).count();
        let skipped = results.iter().filter(|r| matches!(r.outcome, Outcome::Skip { .. })).count();
        Self { total, passed, failed, skipped, results }
    }

    pub fn write_json(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))?;
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}
```

- [ ] **Step 2: Expose conformance module**

`crates/quench-runtime/src/conformance/mod.rs`:

```rust
pub mod report;
```

`crates/quench-runtime/src/lib.rs`:

```rust
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
```

- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/src/conformance/ crates/quench-runtime/src/lib.rs
git commit -m "feat(conformance): shared report types"
```

---

## Task 3: Implement test262 runner (Rust-only helpers)

Use the design in `docs/superpowers/specs/2026-06-28-unified-conformance-harness-design.md`.

**Files:**
- Create: `crates/quench-runtime/src/conformance/test262/mod.rs`
- Create: `crates/quench-runtime/src/conformance/test262/metadata.rs`
- Create: `crates/quench-runtime/src/conformance/test262/helpers.rs`

Implement the native helper functions and runner exactly as described in the test262 plan, but inside `src/conformance/test262/`.

- [ ] **Step 1: Implement metadata parser**
- [ ] **Step 2: Implement native helpers**
- [ ] **Step 3: Implement runner with skip policy**
- [ ] **Step 4: Add integration test subset**
- [ ] **Step 5: Commit**

```bash
git add crates/quench-runtime/src/conformance/test262/ crates/quench-runtime/tests/test262.rs
git commit -m "feat(conformance): test262 runner with Rust native helpers"
```

---

## Task 4: Implement TypeScript baseline runner

**Files:**
- Create: `crates/quench-runtime/src/conformance/typescript/mod.rs`
- Create: `crates/quench-runtime/src/conformance/typescript/directives.rs`
- Create: `crates/quench-runtime/src/conformance/typescript/baseline.rs`
- Create: `crates/quench-runtime/src/conformance/typescript/helpers.rs`
- Create: `crates/quench-runtime/src/conformance/typescript/skip.rs`

- [ ] **Step 1: Parse TypeScript directives**

```rust
// crates/quench-runtime/src/conformance/typescript/directives.rs
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Directives {
    pub target: Option<String>,
    pub module: Option<String>,
    pub jsx: Option<String>,
    pub no_emit: bool,
    pub emit_declaration_only: bool,
    pub filenames: Vec<String>,
    pub raw: HashMap<String, String>,
}

impl Directives {
    pub fn parse(source: &str) -> Self { ... }
}
```

- [ ] **Step 2: Implement skip rules**

`skip.rs`: return `Option<String>` reason for unsupported cases based on directives and path.

- [ ] **Step 3: Locate baseline JS files**

`baseline.rs`: given `tests/cases/conformance/<category>/<case>.ts`, find `tests/baselines/reference/<case>.js` and config-specific variants. Return `Vec<(filename, js_code)>`.

- [ ] **Step 4: TS emit helpers as native functions**

`helpers.rs`: `__extends`, `__assign`, `__awaiter`, `__decorate`, `__importStar`, `__importDefault` implemented in Rust and registered in the `Context`.

- [ ] **Step 5: Single-case runner**

```rust
pub fn run_case(ctx: &mut Context, path: &Path) -> Outcome {
    let source = fs::read_to_string(path)?;
    let directives = Directives::parse(&source);
    if let Some(reason) = skip::should_skip(path, &directives) {
        return Outcome::Skip { reason };
    }
    let units = split_units(&source, &directives);
    let baselines = baseline::load_baselines(path)?;
    for (i, (_name, js)) in baselines.iter().enumerate() {
        if let Err(e) = ctx.eval(js) {
            return Outcome::Fail { error: format!("unit {i}: {e:?}") };
        }
    }
    Outcome::Pass
}
```

- [ ] **Step 6: Directory walker and report writer**

Walk `tests/typescript/tests/cases/conformance/` and collect results.

- [ ] **Step 7: Commit**

```bash
git add crates/quench-runtime/src/conformance/typescript/
git commit -m "feat(conformance): TypeScript baseline-JS runner"
```

---

## Task 5: Wire unified integration test and xtask

**Files:**
- Create: `crates/quench-runtime/tests/conformance.rs`
- Modify: `xtask/src/main.rs`
- Modify: `scripts/run_tests.sh`

- [ ] **Step 1: Write unified entry**

```rust
// crates/quench-runtime/tests/conformance.rs
use quench_runtime::conformance::report::Report;
use quench_runtime::conformance::test262;
use quench_runtime::conformance::typescript;
use std::path::Path;

#[test]
#[ignore = "requires conformance submodules"]
fn conformance_test262() {
    let report = test262::run_suite(Path::new("tests/test262/test"), Some("language/expressions")).unwrap();
    report.write_json(Path::new("target/test262_report.json")).unwrap();
    eprintln!("test262: {}/{}/{}", report.passed, report.failed, report.skipped);
}

#[test]
#[ignore = "requires conformance submodules"]
fn conformance_typescript_baseline() {
    let report = typescript::run_baseline_suite(Path::new("tests/typescript/tests/cases/conformance")).unwrap();
    report.write_json(Path::new("target/conformance_report.json")).unwrap();
    eprintln!("typescript baseline: {}/{}/{}", report.passed, report.failed, report.skipped);
}
```

- [ ] **Step 2: Add xtask command**

In `xtask/src/main.rs`:

```rust
"test-conformance" => {
    cmd!(sh, "cargo test -p quench-runtime --test conformance -- --ignored")
        .run()
        .context("conformance harness failed")?;
}
```

- [ ] **Step 3: Update `scripts/run_tests.sh`**

```bash
test-conformance)
    cargo run -p xtask -- test-conformance
    ;;
```

- [ ] **Step 4: Commit**

```bash
git add crates/quench-runtime/tests/conformance.rs xtask/src/main.rs scripts/run_tests.sh
git commit -m "feat(conformance): unified integration test and xtask wiring"
```

---

## Task 6: Implement source-direct TS mode (phase 2)

**Files:**
- Modify: `crates/quench-runtime/src/conformance/typescript/mod.rs`
- Modify: `crates/quench-runtime/tests/conformance.rs`

- [ ] **Step 1: Add source-direct runner**

```rust
pub fn run_source_suite(root: &Path) -> Result<Report, JsError> { ... }
```

- [ ] **Step 2: Split multi-file cases**

Implement `split_units()` for `// @filename:` markers.

- [ ] **Step 3: Handle module cases**

Use `eval_ts_module` for files with `@module` directive.

- [ ] **Step 4: Add hybrid mode**

Try source-direct first; fall back to baseline JS on failure.

- [ ] **Step 5: Commit**

```bash
git add crates/quench-runtime/src/conformance/typescript/ crates/quench-runtime/tests/conformance.rs
git commit -m "feat(conformance): TypeScript source-direct and hybrid modes"
```

---

## Task 7: Add pass-rate gate

**Files:**
- Modify: `crates/quench-runtime/tests/conformance.rs`

- [ ] **Step 1: Read `MIN_PASS_RATE` env var**

```rust
fn check_pass_rate(name: &str, report: &Report) {
    if let Ok(min) = std::env::var("MIN_PASS_RATE") {
        let min: f64 = min.parse().expect("MIN_PASS_RATE must be a float");
        let rate = report.passed as f64 / report.total as f64;
        assert!(
            rate >= min,
            "{name} pass rate {rate} below minimum {min}"
        );
    }
}
```

- [ ] **Step 2: Call in each conformance test**
- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/tests/conformance.rs
git commit -m "feat(conformance): MIN_PASS_RATE local gate"
```

---

## Task 8: Fix runtime bugs with regression tests

**Files:**
- Various `crates/quench-runtime/src/` files
- Create: `crates/quench-runtime/tests/regressions/`

For each unique failure from either harness:

- [ ] **Step 1: Write focused Rust regression test.**
- [ ] **Step 2: Fix the runtime bug.**
- [ ] **Step 3: Run `cargo test -p quench-runtime`.**
- [ ] **Step 4: Commit.**

Repeat until pass rate stabilizes.

---

## Task 9: Update documentation and task index

**Files:**
- Modify: `docs/conformance.md`
- Modify: `EXECUTE.md`
- Modify: `tasks/index.json`
- Create: `docs/test262.md`

- [ ] **Step 1: Rewrite `docs/conformance.md`** to describe the Rust harness, not the TypeScript runner.
- [ ] **Step 2: Add or update `docs/test262.md`** with run instructions.
- [ ] **Step 3: Update `EXECUTE.md`** to mention both conformance suites.
- [ ] **Step 4: Add task entries** for test262 and TypeScript harnesses and regression-fix batches.
- [ ] **Step 5: Commit**

```bash
git add docs/ tasks/index.json EXECUTE.md
git commit -m "docs: conformance harness usage and task tracking"
```

---

## Verification

After all tasks:

```bash
git submodule update --init tests/test262 tests/typescript
./scripts/run_tests.sh test-conformance
cargo test -p quench-runtime
```

All commands must run with timeouts.
