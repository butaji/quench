# Stage 01 — test/language/literals

**Status:** pending · **Path:** `test/language/literals` (534 tests) · unlocked once stage 00 reaches 100%.

```bash
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Workflow — unit tests first, enforced, no exceptions

**You do not debug. You do not guess. You write a failing unit test first.
Every. Single. Time. No exceptions.**

A failing literals case enters the codebase through one gate: a `#[test]` that
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
   `crates/quench-runtime/tests/`) that exercises the exact literal-parsing
   behavior the case asserts. Mirror the style in `src/parser.rs`'s `mod tests`
   and `src/lower/mod.rs`'s `mod tests`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. If it doesn't fail, or fails for the wrong
   reason, delete the test and write a better one. Do not proceed. You do not
   understand the bug yet.
3. **Fix** — minimal change so the unit test passes. Nothing else. No
   opportunistic refactors.
4. **Verify** — re-run the unit test, the module's full suite, then stage 1.
   `cargo fmt` / `cargo clippy --all-targets` until both are clean. Linter
   warnings block the fix from being "done".
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

`tests/test262.rs` is never edited.