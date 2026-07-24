# Stage 20 — test/language/statements/do-while

**Status:** done · **Path:** `test/language/statements/do-while` ·
**36 tests** · **36 pass / 0 fail (100%)** as of 2026-07-23.

```bash
TEST262_STAGE=20 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 | 29 | 7 | 80.6% | Baseline |
| 2026-07-23 | **36** | **0** | **100%** | do-while completion/hoisting/TCO; string split literal |

## Fixes landed

1. **Var hoisting in do-while** — `collect_var_names_recursive` recurses `DoWhile`/`Labeled`; `eval_do_while_impl` calls `predeclare_var` on body.
2. **Break completion** — return body completion value on break (cptn-abrupt-empty, S12.6.1_A5).
3. **Tail call in do-while body** — `eval_function_body` detects tail `return` inside last-stmt `DoWhile`.
4. **`String.prototype.split` string separator** — literal split, not `Regex::new('.')` (fixes S12.6.1_A8 decimal detection).

## Reproducers kept

- `eval::statement::tests::do_while_statement::do_while_with_nested_labeled_break`
- `eval::statement::tests::do_while_statement::do_while_break_returns_block_completion`
- `builtins::regex::string_methods::tests::split_string_separator_is_literal_not_regex`
- `interpreter::helpers::tests::test_collect_var_names_in_do_while`
