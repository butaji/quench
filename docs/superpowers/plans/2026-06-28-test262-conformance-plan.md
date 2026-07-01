> **I'm using the writing-plans skill to create the implementation plan.**

# test262 Conformance Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the official test262 ECMAScript conformance suite as a submodule and create a harness that runs selected test262 tests against `quench-runtime`, reports results, and drives regression tests for bugs found.

**Architecture:** A new integration test `crates/quench-runtime/tests/test262.rs` uses a helper module (`src/test262/`) to walk the submodule, parse frontmatter, inject minimal harness helpers, evaluate tests through `Context::eval`, and write a JSON report. Tests are skipped by feature/flag until the runtime supports them.

**Tech Stack:** Rust, `serde_yaml`, `walkdir`, `quench-runtime`, test262 harness files.

---

## Task 1: Add test262 submodule and register it

**Files:**
- Modify: `.gitmodules`
- Modify: `crates/quench-runtime/Cargo.toml`

- [ ] **Step 1: Add submodule entry**

```ini
[submodule "tests/test262"]
    path = tests/test262
    url = https://github.com/tc39/test262.git
```

- [ ] **Step 2: Initialize submodule (manual / CI)**

Run:
```bash
git submodule add https://github.com/tc39/test262.git tests/test262
git submodule update --init tests/test262
```

- [ ] **Step 3: Add dev-dependencies**

In `crates/quench-runtime/Cargo.toml` under `[dev-dependencies]`:

```toml
walkdir = "2"
serde_yaml = "0.9"
```

- [ ] **Step 4: Commit**

```bash
git add .gitmodules tests/test262 crates/quench-runtime/Cargo.toml
git commit -m "chore: add test262 submodule and harness dev-deps"
```

---

## Task 2: Parse test262 frontmatter

**Files:**
- Create: `crates/quench-runtime/src/test262/metadata.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/quench-runtime/tests/test262_metadata.rs`:

```rust
use quench_runtime::test262::metadata::Test262Metadata;

#[test]
fn parse_basic_frontmatter() {
    let src = r#"/*---
description: addition returns a number
flags: [onlyStrict]
includes: [assert.js, sta.js]
negative:
  phase: runtime
  type: ReferenceError
---*/
1 + 1;
"#;
    let meta = Test262Metadata::parse(src).unwrap();
    assert_eq!(meta.description, Some("addition returns a number".to_string()));
    assert!(meta.flags.contains(&"onlyStrict".to_string()));
    assert_eq!(meta.includes, vec!["assert.js", "sta.js"]);
    assert_eq!(meta.negative.as_ref().map(|n| n.phase.as_str()), Some("runtime"));
    assert_eq!(meta.negative.as_ref().map(|n| n.typ.as_str()), Some("ReferenceError"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p quench-runtime --test test262_metadata -- --nocapture
```

Expected: FAIL with "module `test262` not found" or similar.

- [ ] **Step 3: Implement minimal metadata parser**

Create `crates/quench-runtime/src/test262/metadata.rs`:

```rust
use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Negative {
    pub phase: String,
    #[serde(rename = "type")]
    pub typ: String,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Test262Metadata {
    pub description: Option<String>,
    pub esid: Option<String>,
    pub info: Option<String>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    pub negative: Option<Negative>,
}

impl Test262Metadata {
    pub fn parse(source: &str) -> Option<Self> {
        let start = source.find("/*---")? + 5;
        let end = source.find("---*/")?;
        let yaml = &source[start..end];
        serde_yaml::from_str(yaml).ok()
    }
}
```

- [ ] **Step 4: Expose module**

Add to `crates/quench-runtime/src/lib.rs`:

```rust
#[cfg(any(test, feature = "test262"))]
pub mod test262;
```

Create `crates/quench-runtime/src/test262/mod.rs`:

```rust
pub mod metadata;
```

- [ ] **Step 5: Run test to verify it passes**

```bash
cargo test -p quench-runtime --test test262_metadata -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/quench-runtime/src/test262/ crates/quench-runtime/tests/test262_metadata.rs crates/quench-runtime/src/lib.rs crates/quench-runtime/Cargo.toml
git commit -m "feat(test262): parse test262 frontmatter metadata"
```

---

## Task 3: Build minimal harness helpers

**Files:**
- Create: `crates/quench-runtime/src/test262/harness.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/quench-runtime/tests/test262_harness.rs`:

```rust
use quench_runtime::{Context, Value};
use quench_runtime::test262::harness::inject_harness;

#[test]
fn harness_assert_same_value_passes() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx).unwrap();
    let result = ctx.eval("assert.sameValue(1 + 1, 2, 'addition');");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn harness_assert_same_value_fails() {
    let mut ctx = Context::new().unwrap();
    inject_harness(&mut ctx).unwrap();
    let result = ctx.eval("assert.sameValue(1 + 1, 3, 'addition');");
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p quench-runtime --test test262_harness -- --nocapture
```

Expected: FAIL with "assert is not defined".

- [ ] **Step 3: Implement harness injection**

Create `crates/quench-runtime/src/test262/harness.rs`:

```rust
use crate::{Context, Value, JsError};

pub const HARNESS_ASSERT: &str = r#"
function Test262Error(message) { this.message = message || ""; }
function $DONOTEVALUATE() { throw new Test262Error("This statement should not be evaluated."); }
function assert(mustBeTrue, message) { if (mustBeTrue !== true) throw new Test262Error(message); }
assert.sameValue = function (a, b, message) { if (a !== b) throw new Test262Error(message); };
assert.notSameValue = function (a, b, message) { if (a === b) throw new Test262Error(message); };
assert.throws = function (ExpectedError, fn, message) {
  try { fn(); } catch (e) {
    if (e instanceof ExpectedError || String(e).includes(ExpectedError.name)) return;
    throw new Test262Error(message);
  }
  throw new Test262Error(message);
};
"#;

pub const HARNESS_DONE: &str = r#"
function $DONE(error) { if (error) throw error; }
"#;

pub fn inject_harness(ctx: &mut Context) -> Result<(), JsError> {
    ctx.eval(HARNESS_ASSERT)?;
    ctx.eval(HARNESS_DONE)?;
    Ok(())
}
```

- [ ] **Step 4: Update module entry**

Edit `crates/quench-runtime/src/test262/mod.rs`:

```rust
pub mod metadata;
pub mod harness;
```

- [ ] **Step 5: Run test to verify it passes**

```bash
cargo test -p quench-runtime --test test262_harness -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/quench-runtime/src/test262/harness.rs crates/quench-runtime/tests/test262_harness.rs crates/quench-runtime/src/test262/mod.rs
git commit -m "feat(test262): inject minimal assert/sta harness helpers"
```

---

## Task 4: Implement runner with skip policy and reporting

**Files:**
- Create: `crates/quench-runtime/src/test262/runner.rs`
- Create: `crates/quench-runtime/tests/test262.rs`

- [ ] **Step 1: Define unsupported feature/flag lists**

In `crates/quench-runtime/src/test262/runner.rs`:

```rust
const SKIP_FEATURES: &[&str] = &[
    "Promise", "async-functions", "generators", "class", "BigInt",
    "Proxy", "Reflect", "WeakMap", "WeakSet", "TypedArray", "RegExp",
    "Symbol", "Symbol.iterator", "default-parameters", "destructuring-binding",
    "spread", "template-literals", "optional-chaining", "private-fields",
];

const SKIP_FLAGS: &[&str] = &["module", "async", "CanBlockIsFalse", "CanBlockIsTrue"];
```

- [ ] **Step 2: Implement run/skip decision**

```rust
use crate::{Context, JsError};
use crate::test262::metadata::Test262Metadata;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TestOutcome {
    Pass,
    Fail { error: String },
    Skip { reason: String },
}

#[derive(Debug, serde::Serialize)]
pub struct TestResult {
    pub path: PathBuf,
    pub outcome: TestOutcome,
}

pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    for flag in &meta.flags {
        if SKIP_FLAGS.contains(&flag.as_str()) {
            return Some(format!("unsupported flag: {flag}"));
        }
    }
    for feat in &meta.features {
        if SKIP_FEATURES.iter().any(|s| s.eq_ignore_ascii_case(feat)) {
            return Some(format!("unsupported feature: {feat}"));
        }
    }
    None
}
```

- [ ] **Step 3: Implement single-test execution**

```rust
use crate::test262::harness::inject_harness;
use std::fs;

pub fn run_test_file(ctx: &mut Context, path: &Path) -> TestOutcome {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return TestOutcome::Fail { error: e.to_string() },
    };
    let meta = Test262Metadata::parse(&source).unwrap_or_default();
    if let Some(reason) = should_skip(&meta) {
        return TestOutcome::Skip { reason };
    }

    let body = if meta.flags.contains(&"onlyStrict".to_string()) {
        format!("\"use strict\";\n{source}")
    } else {
        source
    };

    let result = ctx.eval(&body);

    match (&meta.negative, result) {
        (None, Ok(_)) => TestOutcome::Pass,
        (None, Err(e)) => TestOutcome::Fail { error: format!("{e:?}") },
        (Some(neg), Err(e)) => {
            let msg = format!("{e:?}");
            if msg.contains(&neg.typ) {
                TestOutcome::Pass
            } else {
                TestOutcome::Fail { error: format!("expected {} but got {msg}", neg.typ) }
            }
        }
        (Some(neg), Ok(_)) => TestOutcome::Fail { error: format!("expected {} but succeeded", neg.typ) },
    }
}
```

- [ ] **Step 4: Implement directory walker and report writer**

```rust
use walkdir::WalkDir;
use std::collections::HashMap;

#[derive(Debug, serde::Serialize)]
pub struct Test262Report {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<TestResult>,
}

pub fn run_suite(root: &Path, subset: Option<&str>) -> Result<Test262Report, JsError> {
    let mut ctx = Context::new()?;
    inject_harness(&mut ctx)?;

    let start_dir = subset.map(|s| root.join(s)).unwrap_or_else(|| root.to_path_buf());
    let mut results = Vec::new();

    for entry in WalkDir::new(&start_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        let outcome = run_test_file(&mut ctx, path);
        results.push(TestResult { path: path.to_path_buf(), outcome });
    }

    let total = results.len();
    let passed = results.iter().filter(|r| matches!(r.outcome, TestOutcome::Pass)).count();
    let failed = results.iter().filter(|r| matches!(r.outcome, TestOutcome::Fail { .. })).count();
    let skipped = results.iter().filter(|r| matches!(r.outcome, TestOutcome::Skip { .. })).count();

    Ok(Test262Report { total, passed, failed, skipped, results })
}

pub fn write_report(report: &Test262Report) -> Result<(), std::io::Error> {
    let target_dir = Path::new("target");
    std::fs::create_dir_all(target_dir)?;
    std::fs::write(
        target_dir.join("test262_report.json"),
        serde_json::to_string_pretty(report)?,
    )
}
```

- [ ] **Step 5: Write the integration test entry**

Create `crates/quench-runtime/tests/test262.rs`:

```rust
use quench_runtime::test262::runner::{run_suite, write_report};
use std::path::Path;

#[test]
#[ignore = "requires test262 submodule"]
fn test262_subset() {
    let root = Path::new("tests/test262/test");
    let report = run_suite(root, Some("language/expressions")).expect("suite run failed");
    write_report(&report).expect("report write failed");
    eprintln!("test262: total={} passed={} failed={} skipped={}", report.total, report.passed, report.failed, report.skipped);
    // Do not fail the build while the harness is being tuned.
}
```

- [ ] **Step 6: Add `serde` derives to outcomes**

Add `serde = { version = "1", features = ["derive"] }` to `crates/quench-runtime/Cargo.toml` `[dependencies]` if not already present.

- [ ] **Step 7: Expose runner module**

Edit `crates/quench-runtime/src/test262/mod.rs`:

```rust
pub mod metadata;
pub mod harness;
pub mod runner;
```

- [ ] **Step 8: Run harness on a tiny subset**

```bash
git submodule update --init tests/test262
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
```

Expected: runs, prints counts, writes `target/test262_report.json`.

- [ ] **Step 9: Commit**

```bash
git add crates/quench-runtime/src/test262/ crates/quench-runtime/tests/test262.rs crates/quench-runtime/Cargo.toml
git commit -m "feat(test262): runner, skip policy, and JSON report"
```

---

## Task 5: Wire xtask command for test262

**Files:**
- Modify: `xtask/src/main.rs`

- [ ] **Step 1: Add `test-test262` subcommand**

Locate the existing `test-conformance` command in `xtask/src/main.rs` and add a parallel branch:

```rust
"test-test262" => {
    cmd!(sh, "cargo test -p quench-runtime --test test262 -- --ignored")
        .run()
        .context("test262 harness failed")?;
}
```

- [ ] **Step 2: Update `scripts/run_tests.sh`**

Add:

```bash
test-test262)
    cargo run -p xtask -- test-test262
    ;;
```

- [ ] **Step 3: Verify**

```bash
./scripts/run_tests.sh test-test262
```

Expected: harness runs with timeout.

- [ ] **Step 4: Commit**

```bash
git add xtask/src/main.rs scripts/run_tests.sh
git commit -m "chore: wire test262 harness to xtask and run_tests.sh"
```

---

## Task 6: Expand subset and stabilize pass rate

**Files:**
- Modify: `crates/quench-runtime/src/test262/runner.rs`

- [ ] **Step 1: Add more safe directories**

Change the integration test to run multiple safe directories:

```rust
let subsets = &["language/expressions", "language/statements", "built-ins/Array"];
let mut all_results = Vec::new();
for subset in subsets {
    let report = run_suite(root, Some(subset)).expect("suite run failed");
    all_results.extend(report.results);
}
```

- [ ] **Step 2: Add expected-failure mechanism (optional)**

If failures are known and tracked, add an `expected_failures: HashSet<PathBuf>` loaded from `crates/quench-runtime/tests/test262_expected_failures.json` so CI can alert when a previously-passing test starts failing.

- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/tests/test262.rs
git commit -m "feat(test262): expand subset coverage"
```

---

## Task 7: Fix runtime bugs found by test262 with regression tests

**Files:**
- Various `crates/quench-runtime/src/` files
- Create: `crates/quench-runtime/tests/regressions/`

For each unique failure:

- [ ] **Step 1: Write a focused failing regression test** in `crates/quench-runtime/tests/regressions/test262_<name>.rs`.
- [ ] **Step 2: Fix the runtime bug** in the smallest relevant source file.
- [ ] **Step 3: Run `cargo test -p quench-runtime`** to confirm the regression test passes and no existing tests break.
- [ ] **Step 4: Commit** with message `fix(runtime): <description> (test262 regression)`.

Repeat until the harness pass rate stops improving or the remaining failures are all covered by skip/expected-failure rules.

---

## Task 8: Document and update task index

**Files:**
- Create: `docs/test262.md`
- Modify: `tasks/index.json`
- Modify: `EXECUTE.md`

- [ ] **Step 1: Write `docs/test262.md`** covering:
  - How to initialize the submodule.
  - How to run the harness (`./scripts/run_tests.sh test-test262`).
  - Skip policy and how to remove a feature from the skip list.
  - How to add a regression test for a found bug.

- [ ] **Step 2: Add task entries to `tasks/index.json`** for:
  - test262 submodule + harness.
  - Runtime fixes discovered by test262 (one entry per batch).

- [ ] **Step 3: Update `EXECUTE.md`** to list `test262` alongside the TypeScript conformance harness.

- [ ] **Step 4: Commit**

```bash
git add docs/test262.md tasks/index.json EXECUTE.md
git commit -m "docs: test262 harness usage and task tracking"
```

---

## Verification

After all tasks:

```bash
git submodule update --init tests/test262
./scripts/run_tests.sh test-test262
cargo test -p quench-runtime
```

All commands must run with the timeout wrapper. The harness should produce `target/test262_report.json` and should not break the existing test suite.
