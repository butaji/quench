# Stage 01 — test/language/literals

**Status:** pending · **Path:** `test/language/literals` (534 tests) · unlocked once stage 00 reaches 100%.

```bash
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Workflow — unit tests first, always

**Never debug by inspection. Never patch on a hunch. Always write a failing test first.**

For every failing literals case:

1. **Reproduce** — add a `#[test]` in the relevant module's `mod tests` (or
   `crates/quench-runtime/tests/`) that exercises the exact literal-parsing
   behavior the case asserts. Mirror the style in `src/parser.rs`'s `mod tests`
   and `src/lower/mod.rs`'s `mod tests`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. Refine the test before touching production.
3. **Fix** — minimal change so the unit test passes.
4. **Verify** — re-run the unit test, the module's full suite, then stage 1.
   `cargo fmt` / `cargo clippy` before declaring done.
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

No `println!`-driven archaeology. No speculative rewrites.
`tests/test262.rs` is never edited.