# Stage 00 — test/harness

**Status:** in_progress · **Path:** `test/harness` (116 tests) · 100% target before stage 01.

```bash
TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Workflow — unit tests first, enforced, no exceptions

**You do not debug. You do not guess. You write a failing unit test first.
Every. Single. Time. No exceptions.**

A failing harness case enters the codebase through one gate: a `#[test]` that
asserts the exact behavior, committed *before* any production change. If you
cannot express the behavior as a unit test, you do not understand it yet, and
you are not allowed to touch production code.

### Forbidden

- `println!` / `dbg!` archaeology. **Never.**
- Reading code until it "looks wrong" and patching. **Never.**
- "Let me try this" speculative edits. **Never.**
- Refactors done "while I'm here" without a test. **Never.**
- Skipping the failing-test step "just this once". **Never.**
- Editing `tests/test262.rs` or anything under `tests/test262/`. **Never.**

### Mandatory cycle, in order

1. **Reproduce** — add a `#[test]` in the relevant module's `mod tests` (or
   `crates/quench-runtime/tests/`) that exercises the exact JS/Rust behavior the
   harness case is asserting. Mirror the style in
   `src/eval/string_methods.rs`, `src/builtins/map.rs`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the harness case. If it doesn't fail, or fails for the wrong
   reason, delete the test and write a better one. Do not proceed. You do not
   understand the bug yet.
3. **Fix** — minimal change to production code so the unit test passes. Nothing
   else. No opportunistic refactors.
4. **Verify** — re-run the unit test, the module's whole test suite, then
   stage 0. `cargo fmt` / `cargo clippy --all-targets` until both are clean.
   Linter warnings block the fix from being "done".
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

The conformance run in `tests/test262.rs` is never edited.

## Active failures

- #21 `assert-throws-same-realm.js` — `Expected a Test262Error, but no error was thrown.`
  - **Next step:** write a `#[test]` that reproduces the missing throw in
    `src/test262/harness/` (or the relevant host/assert path), watch it fail
    with the same message, then fix.