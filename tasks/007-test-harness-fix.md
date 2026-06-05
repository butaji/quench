# Task 007: Fix Test Harness Script

## Problem

The original `test_ink_parity_unified.sh` script had issues that caused it to exit silently with code 1 without producing any output. This was due to:

1. Incorrect script flow - after `check_deps` completed, the script would exit instead of calling `run_tests`
2. Complex function definitions that weren't being parsed correctly
3. No proper debugging output to identify the issue

## Solution

Created a new `run_parity_tests.sh` script that:

1. Uses `set -euo pipefail` for proper error handling
2. Has a clear, linear structure
3. Provides proper feedback during execution
4. Works correctly on macOS (no GNU-specific commands)

## Features

- Tests all 88 Ink examples
- Compares Deno (reference) vs runts dev (HIR) output
- Calculates similarity percentage
- Normalizes output for fair comparison (strips ANSI, normalizes whitespace)
- Supports specific examples selection
- Supports dry-run and list modes
- Clean exit codes

## Results

All 88 examples now pass with 100% Deno-HIR similarity:

```
==============================================
  INK PARITY TEST HARNESS
==============================================

Testing 88 examples...

[ink-absolute] ✓ D-H:100%
[ink-align-self] ✓ D-H:100%
...
[ink-z-index] ✓ D-H:100%

==============================================
  RESULTS: Passed=88 Failed=0
==============================================
```

## Files Changed

- Created: `run_parity_tests.sh` - New working test harness
- Modified: `crates/runts-ink/tests/ink_parity_harness_tests.rs` - Fixed complexity issues

## Status: COMPLETED
