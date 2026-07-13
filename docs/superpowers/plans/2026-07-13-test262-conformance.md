# Incremental test262 Conformance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a verified, commit-per-step loop that advances the staged test262 runner, removes every existing skip, and ultimately executes the complete submodule with zero failures under Rust source limits.

**Architecture:** Preserve the current working tree and use the existing fail-fast checkpoint as the semantic frontier. Every behavior change starts with a focused regression test, receives the minimum parser/runtime fix, passes local quality gates, and advances the runner before commit. Parallel agents may investigate independent failures, but only one implementation stream edits shared runtime files.

**Tech Stack:** Rust, Cargo, OXC parser, Quench tree-walking interpreter, test262 submodule, Clippy, rustfmt, dependency-free repository scripts.

---

## Scope and file responsibilities

This objective spans many ECMAScript subsystems, so this document is the executable control plan rather than a speculative list of future language implementations. Each observed failure produces a small follow-up task with exact test and implementation code determined from that test's contract.

Initial files:

- Preserve and validate: all currently modified files reported by `git status --short`.
- Test frontier: `crates/quench-runtime/.test262_checkpoint`.
- Test runner: `crates/quench-runtime/src/test262/runner.rs`.
- Skip backlog: `crates/quench-runtime/src/test262/skip.rs`.
- Harness tolerance backlog: `crates/quench-runtime/src/test262/harness/mod.rs`.
- Regression tests: the closest existing `#[cfg(test)]` module or `crates/quench-runtime/tests/test262.rs`.
- Lint thresholds: `.clippy.toml`.
- File/function limit checker: `scripts/check-rust-limits.py`.
- CI gate: `.github/workflows/ci.yml`.

Engine files are selected only after a failing regression test identifies the responsible boundary: parsing in `parser.rs`, AST transformation in `lower/`, execution in `eval/`, scope behavior in `env.rs`, value semantics in `value/`, or native objects in `builtins/`.

### Task 1: Protect and validate the existing working tree

**Files:**
- Inspect: all paths printed by `git status --short`
- Preserve: `crates/quench-runtime/.test262_checkpoint`

- [ ] **Step 1: Record the existing changes without altering them**

Run:

```bash
git status --short
git diff --stat
git diff --check
```

Expected: the first two commands enumerate the protected edits; `git diff --check` exits successfully. Do not reset, stash, checkout, or reformat yet.

- [ ] **Step 2: Verify formatting and compilation state**

Run:

```bash
cargo fmt --all -- --check
cargo test -p quench-runtime --no-run
```

Expected: both commands pass. If formatting fails, inspect the affected paths before running `cargo fmt`; do not mix unrelated formatting into a semantic commit.

- [ ] **Step 3: Run focused tests associated with the modified modules**

Run:

```bash
cargo test -p quench-runtime promise
cargo test -p quench-runtime regex
cargo test -p quench-runtime object
cargo test -p quench-runtime number
```

Expected: all selected tests pass. Any failure becomes the first TDD task; do not commit the existing changes while red.

- [ ] **Step 4: Run the complete crate test suite**

Run:

```bash
cargo test -p quench-runtime
```

Expected: all non-ignored tests pass.

- [ ] **Step 5: Run the current Clippy gate**

Run:

```bash
cargo clippy -p quench-runtime --all-targets -- -D warnings
```

Expected: zero warnings. Fix only warnings introduced by or located in the protected edits before proceeding.

- [ ] **Step 6: Commit a coherent existing increment, if one exists**

Review the diff for one behavior cluster, such as the currently modified Promise files:

```bash
git diff -- \
  crates/quench-runtime/src/builtins/promise \
  crates/quench-runtime/src/eval/call.rs
```

If that cluster is coherent and verified, stage those exact paths and use a message naming the observed behavior:

```bash
git add -- \
  crates/quench-runtime/src/builtins/promise \
  crates/quench-runtime/src/eval/call.rs
git commit -m "fix: complete Promise constructor behavior"
```

Expected: one focused commit. Leave unrelated protected edits unstaged. If this example cluster is not coherent, do not commit it; select another cohesive cluster from `git diff` and rerun its focused tests first.

### Task 2: Establish the exact test262 failure frontier

**Files:**
- Read: `crates/quench-runtime/.test262_checkpoint`
- Read: the failing file under `tests/test262/test/`
- Read: required harness files under `tests/test262/harness/`
- Do not modify: any file under `tests/test262/`

- [ ] **Step 1: Run the staged runner from its checkpoint**

Run:

```bash
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Expected: either `FIRST FAILURE` with exact file, stage, index, mode, and reason, or suite completion. The reported file is the sole source of truth; do not infer it from the checkpoint.

- [ ] **Step 2: Read the failing test contract**

Read its frontmatter, body, and every file named by `includes`. Record:

```text
path:
flags:
features:
includes:
negative phase and type:
sloppy, strict, or both:
first observable assertion:
```

Expected: a concrete behavior that can be expressed without changing the test262 source.

- [ ] **Step 3: Dispatch parallel read-only investigations when independent**

Use separate subagents for:

```text
A. Locate the engine path responsible for the observed behavior.
B. Derive a minimal standalone JavaScript reproducer and expected result from the test.
C. Review relevant existing tests and identify the narrowest regression-test location.
```

Expected: independent recommendations only. Agents must not edit shared files during investigation.

### Task 3: Execute one red-green test262 increment

**Files:**
- Test: closest existing Rust test module or `crates/quench-runtime/tests/test262.rs`
- Modify: the engine file identified by Task 2
- Never modify: `tests/test262/**`

- [ ] **Step 1: Add the focused failing regression test**

After Task 2 identifies the exact behavior, add a regression test using the closest existing helper. For example, if the failure is the currently skipped eval scope behavior in `S7.4_A5.js`, add this concrete test to `crates/quench-runtime/tests/test262.rs`:

```rust
#[test]
fn eval_var_does_not_leak_to_outer_scope() {
    let result = eval("var x = 0; eval('var x = 1'); x;");
    assert_eq!(result, "0");
}
```

Here `eval` means the existing helper already used by neighboring tests in that file. If the observed failure is different, use the same test shape but derive the JavaScript source and expected result directly from the reported test; do not predict future semantics in advance.

For a JavaScript exception contract, use the existing error helper or this concrete pattern:

```rust
#[test]
fn symbol_to_number_throws_type_error() {
    let error = eval_error("Number(Symbol())");
    assert!(error.contains("TypeError"));
}
```

If the failure requires test262 harness semantics, write a dedicated integration test using the existing helper pattern in `crates/quench-runtime/tests/test262.rs` rather than copying harness logic.

- [ ] **Step 2: Prove the regression test is red**

Run the exact new test by name. For the eval example:

```bash
cargo test -p quench-runtime --test test262 eval_var_does_not_leak_to_outer_scope -- --nocapture
```

Expected: FAIL for the same observable behavior as the staged runner. If it fails for setup or an unrelated feature, minimize the source until the failure matches.

- [ ] **Step 3: Implement the minimum semantic correction**

Change only the responsible parser, lowering, evaluator, environment, value, or builtin boundary. Reuse existing helpers and error mechanisms. Builtins must return `JsError` through `create_js_error_with_type` and `set_thrown_value`; they must not panic.

Any new or changed function must be no more than 40 physical lines and cognitive complexity 10 or less. If the containing file already exceeds 500 lines, first extract the touched responsibility into a focused sibling module without changing unrelated behavior.

- [ ] **Step 4: Prove the regression test is green**

Run the exact test command from Step 2 again. For the eval example:

```bash
cargo test -p quench-runtime --test test262 eval_var_does_not_leak_to_outer_scope
```

Expected: PASS.

- [ ] **Step 5: Run the affected test262 test through the runner**

Run:

```bash
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Expected: the checkpoint advances to the next test or stage, or the runner reports a new failure after the fixed test. It must not report the same failure.

- [ ] **Step 6: Run quality and regression gates**

Run:

```bash
cargo fmt --all -- --check
cargo clippy -p quench-runtime --all-targets -- -D warnings
cargo test -p quench-runtime
```

Expected: all commands pass with zero warnings.

- [ ] **Step 7: Review and commit only this increment**

For the eval example, review and stage its concrete test and engine files:

```bash
git diff --check
git diff -- \
  crates/quench-runtime/tests/test262.rs \
  crates/quench-runtime/src/eval \
  crates/quench-runtime/src/env.rs
git add -- \
  crates/quench-runtime/tests/test262.rs \
  crates/quench-runtime/src/eval \
  crates/quench-runtime/src/env.rs
git commit -m "test262: isolate eval var declarations"
```

For another observed behavior, use the exact regression and engine paths identified in Task 2 and a message naming that behavior.

Expected: one commit containing the red-green test and minimum implementation. Do not include unrelated protected edits or the checkpoint unless repository policy explicitly tracks checkpoint progress.

### Task 4: Repeat the frontier loop with safe parallelism

**Files:**
- Repeat Task 2 and Task 3 against each newly reported test.

- [ ] **Step 1: Resume immediately after every successful commit**

Run:

```bash
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Expected: the next genuine failure or complete suite.

- [ ] **Step 2: Batch only independent investigation work**

Dispatch parallel subagents for different concerns such as parser behavior, runtime semantics, existing test coverage, and source-limit decomposition. Keep implementation sequential when files overlap.

- [ ] **Step 3: Keep the checkpoint honest**

Never manually advance or delete `crates/quench-runtime/.test262_checkpoint` to bypass a failure. `TEST262_LIMIT=N` may bound a run while preserving checkpoint progress:

```bash
TEST262_LIMIT=10 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Expected: up to ten executed tests and a saved resume position.

- [ ] **Step 4: Commit every verified semantic step**

Repeat Task 3 exactly. Do not combine unrelated test262 behaviors merely because they appeared in the same run.

### Task 5: Remove existing feature and flag skips

**Files:**
- Modify: `crates/quench-runtime/src/test262/skip.rs`
- Test: `crates/quench-runtime/src/test262/skip.rs` test module and relevant runtime test module

- [ ] **Step 1: Add a unit test that exposes one selected skip entry**

Start with the existing `hashbang` feature entry. Add this test to `skip.rs` after implementing hashbang parsing:

```rust
#[test]
fn supported_hashbang_feature_is_not_skipped() {
    let metadata = Test262Metadata {
        features: vec!["hashbang".to_string()],
        ..Test262Metadata::default()
    };
    assert_eq!(should_skip(&metadata), None);
}
```

Use the actual `Test262Metadata` construction style defined in `metadata.rs`; do not add a second metadata parser.

- [ ] **Step 2: Prove the skip test is red**

Run:

```bash
cargo test -p quench-runtime supported_hashbang_feature_is_not_skipped
```

Expected: FAIL because `hashbang` currently returns a skip reason.

- [ ] **Step 3: Remove exactly one supported skip entry**

Delete `"hashbang"` from `SKIP_FEATURES`. Do not replace it with a source/path skip.

- [ ] **Step 4: Run all newly enabled test262 tests for that feature**

Use `Grep` to list every `features: [hashbang]` test and map each path to the stage table in `runner.rs`. Run each affected stage without touching the checkpoint. If the paths are under `test/language/comments`, run:

```bash
TEST262_STAGE=6 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Expected: all hashbang tests execute and pass. If they expose semantic failures, keep the entry removal and complete Task 3 for each failure before commit; never restore the skip as a shortcut.

- [ ] **Step 5: Verify and commit the skip removal**

Run:

```bash
cargo fmt --all -- --check
cargo clippy -p quench-runtime --all-targets -- -D warnings
cargo test -p quench-runtime
git add -- \
  crates/quench-runtime/src/test262/skip.rs \
  crates/quench-runtime/src/parser.rs \
  crates/quench-runtime/tests/test262.rs
git commit -m "test262: enable hashbang tests"
```

Expected: hashbang tests are enabled with no replacement skip. Stage only parser and integration-test paths that actually changed.

- [ ] **Step 6: Repeat until feature and flag arrays are empty**

Repeat Steps 1–5 for every entry in `SKIP_FEATURES` and `SKIP_FLAGS`.

### Task 6: Remove source, path, module, and harness skips

**Files:**
- Modify: `crates/quench-runtime/src/test262/skip.rs`
- Modify: `crates/quench-runtime/src/test262/harness/mod.rs`
- Test: corresponding `#[cfg(test)]` modules and runtime regression locations

- [ ] **Step 1: Turn each source/path skip into a failing unit test**

For each branch in `should_skip_source`, each `SKIP_TEST_PATHS` entry, and each `SKIP_PATH_PREFIXES` entry, add an assertion that the representative source or path is no longer skipped.

Example for a source rule after async support exists:

```rust
#[test]
fn async_source_is_not_skipped() {
    assert_eq!(should_skip_source("async function f() {}"), None);
}
```

Expected before removal: FAIL.

- [ ] **Step 2: Delete exactly one skip rule and run newly exposed tests**

Start with `S7.4_A5.js`: after the eval-scope regression in Task 3 passes, delete its exact entry from `SKIP_TEST_PATHS` and run stage 6:

```bash
TEST262_STAGE=6 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Apply Task 3 to every newly exposed semantic failure until stage 6 passes without this rule.

- [ ] **Step 3: Remove module-flag skipping only after module execution exists**

Add a focused runner test proving metadata with `flags: [module]` reaches module evaluation rather than `should_skip`. Remove the module branch from `should_skip`, then run every stage containing module tests.

- [ ] **Step 4: Remove tolerated harness failures**

Add a harness-loader test asserting `resizableArrayBufferUtils.js` evaluates successfully, then delete its name from `TOLERATED_EVAL_FAILURES`.

Run:

```bash
cargo test -p quench-runtime test262::harness
cargo test -p quench-runtime --test test262
```

Expected: harness loading succeeds without tolerance.

- [ ] **Step 5: Commit each independently verified removal**

For the first path removal:

```bash
git add -- \
  crates/quench-runtime/src/test262/skip.rs \
  crates/quench-runtime/tests/test262.rs \
  crates/quench-runtime/src/eval \
  crates/quench-runtime/src/env.rs
git commit -m "test262: execute S7.4_A5 eval-scope test"
```

Stage only paths that actually changed. Use one commit per source rule, path cluster, module support increment, or harness file.

### Task 7: Enforce Rust source limits without dependencies

**Files:**
- Create: `scripts/check-rust-limits.py`
- Modify: `.clippy.toml`
- Modify: `.github/workflows/ci.yml`
- Modify: Rust files exceeding limits, split by responsibility

- [ ] **Step 1: Write a failing file/function limit checker**

Create `scripts/check-rust-limits.py` with this complete initial implementation:

```python
#!/usr/bin/env python3
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
RUST_ROOT = ROOT / "crates" / "quench-runtime"
FN_START = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+\w+")


def function_lengths(lines: list[str]) -> list[tuple[int, int]]:
    lengths = []
    start = None
    depth = 0
    for number, line in enumerate(lines, 1):
        if start is None and FN_START.match(line):
            start = number
            depth = 0
        if start is not None:
            depth += line.count("{") - line.count("}")
            if depth == 0 and "}" in line:
                lengths.append((start, number - start + 1))
                start = None
    return lengths


def main() -> int:
    failures = []
    for path in sorted(RUST_ROOT.rglob("*.rs")):
        lines = path.read_text(encoding="utf-8").splitlines()
        relative = path.relative_to(ROOT)
        if len(lines) > 500:
            failures.append(f"{relative}: {len(lines)} lines (max 500)")
        for start, length in function_lengths(lines):
            if length > 40:
                failures.append(
                    f"{relative}:{start}: function is {length} lines (max 40)"
                )
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

This conservative checker is intentionally dependency-free. Before CI activation, validate every reported function manually; improve brace handling only with checker unit tests if false positives appear.

- [ ] **Step 2: Run the checker and record the red baseline**

Run:

```bash
python3 scripts/check-rust-limits.py
```

Expected: FAIL listing every Rust file over 500 lines and function over 40 lines.

- [ ] **Step 3: Configure Clippy's function and complexity limits**

Set `.clippy.toml` to include:

```toml
cognitive-complexity-threshold = 10
too-many-lines-threshold = 40
```

Run:

```bash
cargo clippy -p quench-runtime --all-targets -- \
  -D warnings \
  -D clippy::cognitive_complexity \
  -D clippy::too_many_lines
```

Expected initially: FAIL on legacy violations.

- [ ] **Step 4: Eliminate violations in focused structural commits**

For each reported file, split cohesive responsibilities into sibling modules and extract named helper functions. Preserve behavior and run the nearest tests after every extraction. Never combine unrelated files in one structural commit.

Verification per extraction:

```bash
python3 scripts/check-rust-limits.py
cargo fmt --all -- --check
cargo clippy -p quench-runtime --all-targets -- \
  -D warnings \
  -D clippy::cognitive_complexity \
  -D clippy::too_many_lines
cargo test -p quench-runtime
```

Expected: the violation count decreases, with no semantic test regressions.

Commit each reduction using the exact split paths. For example, when splitting Promise static methods:

```bash
git add -- \
  crates/quench-runtime/src/builtins/promise/static_methods.rs \
  crates/quench-runtime/src/builtins/promise/static_methods
git commit -m "refactor: split Promise static method implementations"
```

Stage only paths that exist and changed for that extraction.

- [ ] **Step 5: Activate repository-wide CI enforcement at zero violations**

Add these commands to `.github/workflows/ci.yml`'s lint job after formatting:

```yaml
      - name: Check Rust source limits
        run: python3 scripts/check-rust-limits.py
      - name: Clippy
        run: >-
          cargo clippy --all-targets --
          -D warnings
          -D clippy::cognitive_complexity
          -D clippy::too_many_lines
```

Run the exact commands locally. Expected: PASS.

- [ ] **Step 6: Commit enforcement**

```bash
git add -- scripts/check-rust-limits.py .clippy.toml .github/workflows/ci.yml
git commit -m "ci: enforce Rust source size and complexity limits"
```

Expected: future Rust violations fail CI.

### Task 8: Prove zero skips and full-suite conformance

**Files:**
- Modify if needed: `crates/quench-runtime/src/test262/runner.rs`
- Test: `crates/quench-runtime/src/test262/runner.rs` test module

- [ ] **Step 1: Add an assertion that any skip fails the final suite run**

Introduce a strict runner mode used by the conformance command. Its test must construct a skipped outcome and assert the run is unsuccessful. Keep normal diagnostic counting only if needed for intermediate development; the final command must reject `skipped > 0`.

- [ ] **Step 2: Prove the strict runner test is red, then implement it**

Run the exact new test before and after implementation. Expected: FAIL first, PASS after the minimum runner change.

- [ ] **Step 3: Verify all skip sources are empty**

Run:

```bash
rg 'SKIP_FEATURES|SKIP_FLAGS|SKIP_TEST_PATHS|SKIP_PATH_PREFIXES|should_skip_source|TOLERATED_EVAL_FAILURES' crates/quench-runtime/src/test262
```

Expected: definitions may remain only as empty structures or pass-through functions required by interfaces; no test or harness name is suppressed.

- [ ] **Step 4: Run every test from the submodule**

Run:

```bash
rm -f crates/quench-runtime/.test262_checkpoint
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Expected: every discovered test executes, zero fail, zero skip, zero tolerated harness failure, and the runner removes its checkpoint on completion. Removing the checkpoint here is valid because this is the intentional final full-suite proof, not a bypass.

- [ ] **Step 5: Run all final quality gates**

```bash
python3 scripts/check-rust-limits.py
cargo fmt --all -- --check
cargo clippy -p quench-runtime --all-targets -- \
  -D warnings \
  -D clippy::cognitive_complexity \
  -D clippy::too_many_lines
cargo test -p quench-runtime
```

Expected: all commands pass.

- [ ] **Step 6: Commit the strict completion gate**

```bash
git add -- crates/quench-runtime/src/test262/runner.rs
git commit -m "test262: require complete suite with zero skips"
```

If runner tests are extracted to a sibling file during the 500-line cleanup, stage that exact sibling file too.

Expected: the repository has machine-checked full-suite conformance and lint compliance.

## Iteration contract

Tasks 2–4 are the fast recurring loop. Tasks 5–6 remove the finite skip backlog feature by feature. Task 7 becomes a hard gate only after legacy violations are eliminated. Task 8 is the completion proof. At no point may an implementation advance by editing the test262 submodule, adding a skip, tolerating another harness failure, manually moving the checkpoint past a failing test, or committing red tests.
