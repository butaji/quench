# Stage 17 — test/language/statements/const

**Status:** in_progress · **Path:** `test/language/statements/const` ·
**136 tests** · **113 pass / 23 fail (83.1%)** as of 2026-07-23.

```bash
TEST262_STAGE=17 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 | **113** | **23** | **83.1%** | Baseline after stage 16 complete |

## Top remaining clusters (23)

| Count | Cluster | Fix direction |
|------:|---------|---------------|
| 6 | Destructure null/undefined → TypeError expected | `assign_object_destructuring` / const pattern decl throw on nullish RHS |
| 4 | `ReferenceError: x is not defined` (nested destructure props) | const object-pattern nested binding env |
| 2 | Init throw / eval err not propagated | destructuring initializer exception handling |
| 2 | Object rest (getter / non-enumerable) | rest copy enumerable-only semantics |
| 1 | `fn-name-cover` / class name in destructure init | SetFunctionName for cover names |
| 1 | `const-outer-inner-let-bindings` | block scope shadowing for const/let |
| 1 | `const-invalid-assignment-statement-body-for-of` | for-of const reassignment TypeError |
| 6 | misc single-file | see `tasks/failures-17.json` |
