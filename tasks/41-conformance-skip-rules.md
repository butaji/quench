# Task 41: Tighten conformance skip rules for non-runnable cases

## Status: COMPLETED

### What was done (2026-06-30)

The `should_skip()` function in `conformance.rs` already handled most skip rules. Added comprehensive unit tests:

- `test_skip_rules_noemit` — verifies `@noEmit: true` is detected
- `test_skip_rules_emitdeclarationonly` — verifies `@emitDeclarationOnly: true` is detected
- `test_skip_rules_unsupported_module` — verifies `amd|umd|system|node16|nodenext|none` modules are skipped
- `test_skip_rules_skipped_directory` — verifies cases in `types/`, `interfaces/`, `symbols/`, etc. are skipped

### Current skip rules

| Rule | Reason |
|------|--------|
| `@noEmit: true` | TypeScript produces no JS |
| `@emitDeclarationOnly: true` | Only `.d.ts` output |
| `@module: amd\|umd\|system\|node16\|nodenext\|none` | Unsupported module system |
| `@jsx: react` | JSX react runtime not available |
| Cases in `types/`, `interfaces/`, `symbols/`, `declarationEmit/`, etc. | Type-only tests |
| `.errors.txt` suffix | Diagnostic-only tests |
| `declare var` + `class` in same file | Type-only declaration, stripped |
| `await`/`yield` in class body computed property keys | Async not implemented |
| `get\n*` pattern | Generator accessor shorthand (invalid JS) |

### Files changed

- `crates/quench-runtime/tests/conformance.rs` — 4 new tests

### Verification

```bash
cargo test -p quench-runtime --test conformance test_skip  # ✓ 4 tests pass
```
