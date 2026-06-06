# Task: Fix or Document Stale HIR Test Failures

**Priority:** P1-High  
**Phase:** 2 — Compile + Verification  
**Depends on:** 033

## Problem

`cargo test --bin runts` currently has **113 failures** in enabled test modules:

| Module | Failures | Root Cause |
|--------|----------|------------|
| `spec_modules` | 24 | Tests expect old `ModuleItem::Decl` / `ModuleItem::Export` shape from before `crates/runts-hir` refactor |
| `spec_stdlib` | 12 | `Math.PI`, `Date.now()`, `array.length` parse to `Expr::StaticMember` but quote_codegen panics on `Expr::Invalid` |
| `completeness_parser` | 22 | JSX, optional chaining, class expr, meta props, sequences return `Stmt::Empty` / `Expr::Invalid` — parser converter not implemented |
| `completeness_codegen` | 1 | Spread expression edge case |
| `integration` | 3 | Type-to-rust failures, full transpile failures |
| `parser` (jsx) | 7 | JSX text coalescing, HIR JSON serialization |
| `quote_codegen` | 4 | Panic on `do-while`, `throw`, labeled statements, intersection types |

These failures are **compile-path only** — the dev path (TSX→JS→rquickjs) bypasses HIR entirely. But the compile path cannot be verified without fixing these.

## Decision Matrix

For each failing test, apply ONE of:

| Action | When to use |
|--------|-------------|
| **Fix** | Parser converter missing a feature that compile path needs (e.g. `export default`, `import` statements) |
| **`#[ignore]`** | Feature is intentionally not in compile path scope (e.g. `class` expressions, `with` statement) |
| **Delete** | Test is testing a removed subsystem (e.g. old `ModuleItem` enum shape) |
| **Update assertion** | HIR shape changed legitimately (e.g. `runts_hir::Module` now has `source_path` field) |

## Steps

1. Run `cargo test --bin runts 2>&1 | tee /tmp/failures.txt`.
2. Categorize each failure into the matrix above.
3. Fix batch by batch — e.g. all `spec_modules` failures first, then `spec_stdlib`.
4. For `quote_codegen` panics: replace `panic!("codegen for Invalid expression")` with `return None`.
5. For parser completeness failures: if the converter intentionally skips a feature, document it in `docs/SUPPORTED_SUBSET.md`.

## Acceptance Criteria

- [ ] `cargo test --bin runts` exits 0.
- [ ] No panics on `Expr::Invalid` in quote_codegen.
- [ ] Every `#[ignore]`d test has a reason comment.
- [ ] `docs/SUPPORTED_SUBSET.md` lists any features compile path intentionally skips.
