# Task 021: Re-enable build.rs Linter and Mechanically Fix All 47 Violations

**Priority:** P0-Critical  
**Phase:** 1 — Structural Integrity  
**ETA:** 4–6 hours  
**Depends on:** 020 (build must pass first)

## The Problem

`build.rs` defines strict linting rules:

- File ≤ 500 lines
- Function ≤ 40 lines
- Complexity ≤ 10

These rules are **commented out** because the codebase currently has **47 violations**:

| Violation Type | Count | Worst Offender |
|----------------|-------|----------------|
| File too long | 6 | `hir_runtime.rs` (2,773 lines) |
| Function too long | 11 | `eval_expr` (356 lines) |
| Function too complex | 10 | `eval_expr` (complexity 171) |

Without enforcement, every new commit makes the code harder to change. We are currently **6× over** the file limit and **17× over** the function complexity limit.

## Why This Is P0

- EXECUTE.md requires adding ~40 new features to HIR runtime.
- Adding them to `eval_expr` (356 lines) is malpractice.
- The linter is the only guardrail preventing a 5,000-line god file.

## Steps

### Step 1: Uncomment the linter in `build.rs`

Replace:
```rust
// TODO: Re-enable strict linting after fixing pre-existing violations
// let (violations, files_checked) = run_linter();
```

With:
```rust
let (violations, files_checked) = run_linter();
```

### Step 2: Fix violations mechanically — do NOT rewrite logic

For each violating function, apply **one** of these mechanical transforms:

| Transform | Example |
|-----------|---------|
| Extract match arm into helper | `Expr::Call { .. } => self.eval_call(expr)` |
| Extract repeated block into helper | `if let Value::Number(n) = val { b.prop = *n as u16; }` → `set_num_prop!(b, prop, val)` |
| Split file by domain | `hir_runtime.rs` → `interpreter/expr.rs`, `interpreter/jsx.rs`, `interpreter/hooks.rs`, `interpreter/stdlib.rs`, `interpreter/ink_props.rs` |

**Rule:** Every extracted function must have the exact same semantics. No refactoring of algorithms.

### Step 3: Run `cargo build` after each file

Do not batch-fix 10 files and then build. Fix one file, build, commit mentally, move to next.

### Step 4: Verify green

```bash
cargo build
# build.rs should print:
# runts-lint: 126 file(s) OK
```

## Acceptance Criteria

- [ ] `build.rs` linter is uncommented and runs on every build.
- [ ] `cargo build` passes with 0 linter violations.
- [ ] No function > 40 lines.
- [ ] No file > 500 lines.
- [ ] No function complexity > 10.

## Files to Modify

1. `build.rs` — uncomment linter
2. `src/hir_runtime.rs` — split into `src/interpreter/` modules
3. `src/transpile/hir/quote_codegen.rs` — extract `gen_stmt` match arms
4. `src/commands/build/mod.rs` — extract plugin build into `src/commands/build/plugin_build.rs`
5. `src/commands/build/source_gen.rs` — extract `generate_all` sub-steps
6. `crates/runts-fresh/src/plugin.rs` — extract `extract_handler_methods`
7. `crates/runts-ink/src/render.rs` — extract frame loop / ANSI parser
8. `crates/runts-ink/src/components.rs` — split builder methods
9. `crates/runts-ink/src/js_bridge.rs` — split reconciler message handlers

## Notes

- If a violation is in a `#[cfg(test)]` block, the linter skips `/tests/` but NOT inline tests. Move inline tests to `tests/` files.
- Do not change the linter thresholds. They exist for a reason.
