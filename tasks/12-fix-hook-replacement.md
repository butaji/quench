# Task 12: Fix hook replacement in compiler (substring collision)

## Status: completed

## Date: 2026-06-29

## Problem

The `prefix_hooks` function in `src/compiler/mod.rs` used simple string replacement to prefix React/Ink hooks with `ink.`:

```rust
result = result.replace(hook, &format!("ink.{}", hook));
```

This caused a bug where hook names embedded in other identifiers were incorrectly replaced. For example:
- `MouseApp` contains `useApp` as a substring
- `MouseApp` was transformed to `Moink.useApp`
- This caused a parse error at position 3921..3922

## Solution

Changed `prefix_hooks` to use regex with word boundaries (`\b`) to only match complete hook names:

```rust
fn prefix_hooks(js: &str) -> String {
    let hooks = [...];
    let mut result = js.to_string();
    
    // Build regex pattern with word boundaries
    let all_hooks = hooks.join("|");
    let pattern = format!(r"\b({})\b", all_hooks);
    let re = regex::Regex::new(&pattern).unwrap();
    
    // Protect already-prefixed
    result = result.replace("ink.ink.", "ink.");
    
    // Replace hook names with word boundaries only
    result = re.replace_all(&result, "ink.$0").to_string();
    
    // Clean up any double-prefixing
    result.replace("ink.ink.", "ink.")
}
```

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files Changed

- `Cargo.toml`: Added `regex = "1"` dependency
- `src/compiler/mod.rs`: Rewrote `prefix_hooks` to use regex with word boundaries

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

- All 60+ examples now run successfully (including `mouse-app.tsx` which was failing)
- All 46 quench-runtime unit tests pass
- All 34 main crate tests pass
- All 3 parity tests pass
