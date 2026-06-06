# Task 020: Fix quote_codegen.rs Syntax Error — Unblock the Build

**Priority:** P0-Critical  
**Phase:** 0 — Unblock  
**ETA:** 15 minutes  
**Blocking:** Every other task

## The Problem

`cargo build` fails with a single syntax error:

```
error: unexpected closing delimiter: `}`
   --> src/transpile/hir/quote_codegen.rs:609:5
```

## Steps

1. Open `src/transpile/hir/quote_codegen.rs` around line 609.
2. Inspect the `impl QuoteCodegen` block starting at line 17.
3. Find the brace mismatch. Likely causes:
   - A `fn` body missing `{` or having an extra `}`.
   - An `if`/`match` arm with mismatched braces inside a function.
   - A macro invocation (`quote! {}`) that got mangled.
4. Fix the brace balance so `cargo build` passes.
5. Run `cargo build` and confirm **zero errors**.

## Acceptance Criteria

- [ ] `cargo build` exits 0.
- [ ] `cargo test --no-run` exits 0 (tests compile).
- [ ] No changes to logic — purely syntax fix.
