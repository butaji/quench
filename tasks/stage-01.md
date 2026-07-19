# Stage 01 — test/language/literals

**Status:** in_progress · **Path:** `test/language/literals` (534 tests) · stage 00 done.

```bash
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

On success the runner prints `ALL STAGES COMPLETE — Stage 1: 534/534`; that line drives the CI summary job and is the gate to advance to stage 02.

## Workflow

See `AGENTS.md` → "Workflow: unit tests, not guesswork" — the same cycle
applies to every stage. Reproduce via a `#[test]` next to the parser /
lowering module under inspection, watch it fail, fix minimally, verify,
leave the test in. Do not edit `tests/test262.rs` or anything under
`tests/test262/`.

## Notes for literals specifically

- Test styles to mirror: `src/parser.rs` `mod tests`, `src/lower/mod.rs`
  `mod tests`.
- Numeric / string / regex / template literals touch the parser; null /
  undefined / boolean literals touch the eval value path. Pick the
  matching module before writing the reproducer.

## History

_(populated as cases land)_