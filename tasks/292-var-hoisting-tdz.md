> **Superseded by focused tasks 339 and 340. This task tracks the overall milestone.**

# Task 292: Implement var hoisting and let/const TDZ

## Status: COMPLETED

## Resolution

Focused tasks 339 and 340 landed the required fixes:

- **Task 339** — `var` declarations are now hoisted inside function scope.
- **Task 340** — `let`/`const` TDZ and const-assignment `TypeError` are enforced.

## Verification

```bash
cargo test -p quench-runtime --test var_hoisting_tdz
```

Result: **17/17 tests pass**.

## Exit criteria

- [x] `var_hoisting_tdz.rs` passes 17/17 tests in parallel.
- [x] test262 `language/statements/variable/`, `language/statements/let/`, and `language/statements/const/` subsets are unblocked for incremental conformance work.

## Targets

- **Suite:** `test262`
- **Batch:** 2
- **Target subset:** `tests/test262/test/language/statements/{variable,let,const}/`
- **Blocked by:** 338, 339, 340
- **Exit criteria:** All hoisting/TDZ/const tests pass and this task is closed.
