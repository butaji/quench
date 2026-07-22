# Stage 01 — test/language/literals

**Status:** done · **Path:** `test/language/literals` (534 tests) · stage 00 done.

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
- This stage is pure parser/lowering work — no self-hosted JS builtin
  is touched. The R0 self-hosting pivot (`tasks/refactor-plan.md`) is
  orthogonal and can proceed in parallel without regressing literals.

## History

- BigInt: registered module, literals, comparison (same_value, strict_eq)
- ++ / -- / ||= / &&= / ??=: fixed set_object_property early-return bug
- Regex fast path: bypass OXC for simple literals, single-char cache
- Regex line terminator rejection (ES 11.8.5)
- eval_impl: removed stale duplicate fast path (native eval now throws)
- instanceof SyntaxError: fixed &Context vs &mut Context UB
- Test runner: per-test timeout with "Must be optimized" diagnostic
- 18 core unit tests covering all fixes (++ / eval / instanceof / BigInt)