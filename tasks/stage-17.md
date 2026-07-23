# Stage 17 — test/language/statements/const

**Status:** done · **Path:** `test/language/statements/const` ·
**136 tests** · **136 pass / 0 fail (100%)** as of 2026-07-23.

```bash
TEST262_STAGE=17 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 | 113 | 23 | 83.1% | Baseline after stage 16 complete |
| 2026-07-23 | **136** | **0** | **100%** | Object destructure → `PatternDeclaration`; for-of per-iteration const; SetFunctionName guard |

## Fixes landed

1. **Object destructuring lowering** — route `const {…} = rhs` through `PatternDeclaration` + runtime `assign_object_destructuring` (nullish throw, nested bindings, rest enumerable copy, defaults only on `undefined`).
2. **For-of let/const** — per-iteration lexical scope via `loop_binding` on `Expression::ForOf`; removed const→let hack.
3. **SetFunctionName** — `eval_var_decl` only names functions when `IsAnonymousFunctionDefinition(init)`.
4. **For-loop init scope** — `eval_for` push/pop scope for let/const init bindings.
5. **Const reassignment TypeError** — `assign_to_identifier` uses `create_js_error_with_type`.

## Reproducers kept

- `eval::object::helpers::destructuring::tests::const_empty_object_destructure_null_throws_type_error`
- `eval::statement::tests::const_decl::const_fn_cover_grammar_does_not_set_function_name`
- `eval::statement::tests::const_decl::for_of_const_increment_throws_type_error`
