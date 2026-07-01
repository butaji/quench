# Task 63: Split monolithic runtime files into subdirectories

## Status: In Progress

## Goal

Split the monolithic files in `crates/quench-runtime/src/` into proper subdirectories following the build.rs lint rules and architectural hygiene guidelines.

## Current State

- `builtins.rs`: 1720 lines (mixes Array, Map, Set, Promise, Math, JSON, Error, globals)
- `interpreter.rs`: 1514 lines (eval_expression, eval_statement, call logic)
- `lower.rs`: 1243 lines (swc lowering)
- `value.rs`: 702 lines (Value type, Object model)
- `env.rs`: 256 lines (Environment chain)

## Target Structure

```
crates/quench-runtime/src/
├── builtins/
│   ├── mod.rs (reexports)
│   ├── array.rs
│   ├── map.rs
│   ├── set.rs
│   ├── promise.rs
│   ├── string.rs
│   ├── date.rs
│   ├── number.rs
│   ├── object.rs
│   ├── json.rs
│   ├── math.rs
│   ├── error.rs
│   └── globals.rs
├── interpreter/
│   ├── mod.rs (reexports + main eval functions)
│   ├── eval_expr.rs
│   ├── eval_stmt.rs
│   ├── call.rs
│   └── binary_ops.rs
├── lower/
│   ├── mod.rs (reexports)
│   ├── expr.rs
│   ├── stmt.rs
│   └── decl.rs
├── value/
│   ├── mod.rs (reexports)
│   ├── value.rs
│   ├── object.rs
│   └── error.rs
├── context/
│   └── mod.rs (Context)
├── ast.rs (keep as-is)
├── env.rs (keep as-is)
├── swc_parse.rs (keep as-is)
└── lib.rs (update imports)
```

## Requirements

1. Preserve ALL existing functionality - tests must pass after split
2. Maintain the same public API in lib.rs
3. Each sub-module file should be under 500 lines
4. Functions should be under 40 lines
5. Add #[allow(...)] for legitimate exceptions
6. Run `cargo test -p quench-runtime` to verify

## Approach

1. First create the directory structure with empty mod.rs files
2. Move one module at a time, running tests after each
3. Keep existing function bodies unchanged during the split
4. Update lib.rs to re-export from submodules

## Progress

- [x] Created directory structure
- [ ] Split builtins.rs (1720 lines → ~12 files)
- [ ] Split interpreter.rs (1514 lines → ~5 files)
- [ ] Split lower.rs (1243 lines → ~4 files)
- [ ] Split value.rs (702 lines → ~4 files)
- [ ] Update lib.rs imports
- [ ] Verify all tests pass

## Verification

```bash
cargo test -p quench-runtime
cargo clippy --all-targets
```

## Notes

- This is purely organizational - no functional changes
- The runtime already works correctly
- This enables better maintainability and obeys the build.rs lint rules
