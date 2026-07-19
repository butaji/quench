# Stage 00 — test/harness

**Status:** in_progress · **Path:** `test/harness` (116 tests) · 100% target before stage 01.

```bash
TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Workflow — unit tests first, always

**Never debug by inspection. Never patch on a hunch. Always write a failing test first.**

For every failing harness case:

1. **Reproduce** — add a `#[test]` in the relevant module's `mod tests` (or
   `crates/quench-runtime/tests/`) that exercises the exact JS/Rust behavior the
   harness case is asserting. Mirror the style in
   `src/eval/string_methods.rs`, `src/builtins/map.rs`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the harness case. If it doesn't, refine the test before
   touching production code.
3. **Fix** — minimal change to production code so the unit test passes.
4. **Verify** — re-run the unit test, the module's whole test suite, then
   stage 0. `cargo fmt` / `cargo clippy` before declaring done.
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

No `println!`-driven archaeology. No speculative rewrites. The conformance run
in `tests/test262.rs` is never edited.

## Active failures

- #21 `assert-throws-same-realm.js` — `Expected a Test262Error, but no error was thrown.`
  - **Next step:** write a `#[test]` that reproduces the missing throw in
    `src/test262/harness/` (or the relevant host/assert path), watch it fail
    with the same message, then fix.