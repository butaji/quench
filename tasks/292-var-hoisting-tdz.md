> **Superseded by focused tasks 339 and 340. This task tracks the overall milestone.**

# Task 292: Implement var hoisting and let/const TDZ

## Status: COMPLETED

## Why reopened

The previous implementation was part of the bytecode drift and was reverted. The current committed code does not hoist `var` inside functions and does not enforce `let`/`const` TDZ.

## Exact path forward

Do not implement this directly. Close the following focused tasks first; this task closes automatically when they are done:

1. **Task 339** — Fix `var` hoisting inside function scope.
2. **Task 340** — Fix `let`/`const` TDZ and const assignment `TypeError`.

## Exit criteria

- [ ] `var_hoisting_tdz.rs` passes 17/17 tests in parallel.
- [ ] test262 `language/statements/variable/`, `language/statements/let/`, and `language/statements/const/` subsets improve.

## Targets

- **Suite:** `test262`
- **Batch:** 2
- **Target subset:** `tests/test262/test/language/statements/{variable,let,const}/`
- **Blocked by:** 338, 339, 340
- **Exit criteria:** All hoisting/TDZ/const tests pass and this task is closed.
