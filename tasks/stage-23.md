# Stage 23 — test/language/statements/for

**Status:** in_progress · **Path:** `test/language/statements/for`.

```bash
TEST262_STAGE=23 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Notes |
|------|-------|
| 2026-08-08 | Baseline: 259/385 failing. |

## Top remaining clusters

| Cluster | Count | Fix direction |
|---------|-------|---------------|
| `Expected Test262Error/TypeError/ReferenceError to be thrown` | ~78 | iterator-protocol destructuring error propagation in `eval/object/helpers/destructuring.rs` |
| `sameValue 4 !== undefined` | 2 | for-loop completion value (`cptn-*`) |
| `sameValue "second" !== "first"` | 1 | per-iteration lexical scope for `let` in for-headers (`scope-body-lex-boundary`) |
| `TypeError: Value is not a function` | 2 | per-iteration scope / closure capture (`scope-body/head-lex-open`) |

## Landed fixes

- Destructuring patterns in for-headers: `lower_for_stmt` desugars
  `for (let [x, y] = e; ...)` to a wrapper block (259 → 93).
- For-loop completion value: `eval_for` returns the last non-empty body value
  when the condition becomes false (`cptn-expr-expr-iter`).
- Var hoisting from for-headers: `collect_var_names_recursive` collects
  `ForInit::VarDeclaration` names (Sloppy `S12.6.3_A13/A14`).
- Strict-mode global var in for-header: `eval_for` calls `set_on_global_this`
  for top-level `var` (Sputnik `S12.6.3_*`).
- Multiple declarations: desugar `for (let a = 1, b = 2; ...)` headers
  (`scope-head-lex-open`). Stage 23 → 88 failing.