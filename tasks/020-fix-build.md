# Task 020: Fix quote_codegen.rs Syntax Error — Unblock the Build

**Priority:** P0-Critical  
**Phase:** 0 — Unblock  
**ETA:** 15 minutes  
**Blocking:** Every other task (compile, test, parity harness)

## The Problem

`cargo build` fails with a single syntax error:

```
error: unexpected closing delimiter: `}`
   --> src/transpile/hir/quote_codegen.rs:609:5
```

This means an `impl` block or a function body is missing its opening `{` or has an extra `}`. Because this file is 2,039 lines and was edited during a bulk refactor, a brace was dropped.

## Why This Is P0

- `runts dev` cannot start without a compiled binary.
- The parity harness cannot invoke `cargo build`.
- CI is red.
- **Nobody can verify any change.**

## Steps

1. Open `src/transpile/hir/quote_codegen.rs` around line 609.
2. Inspect the `impl QuoteCodegen` block starting at line 17.
3. Find the mismatch. Likely causes:
   - A `fn` body missing `{` or having an extra `}`.
   - An `if`/`match` arm with mismatched braces inside a function.
   - A macro invocation (`quote! {}`) that got mangled and left a stray `}`.
4. Fix the brace balance so `cargo build` passes.
5. Run `cargo build` and confirm **zero errors**.

## Acceptance Criteria

- [ ] `cargo build` exits 0.
- [ ] `cargo test --no-run` exits 0 (tests compile).
- [ ] No changes to logic — purely syntax fix.

## Notes

- Do NOT refactor anything while fixing this. One line change max.
- If the file is too mangled to eyeball, use `rustfmt` after the fix to normalize braces.
