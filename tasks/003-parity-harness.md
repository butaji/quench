# Task 003: Improve Parity Test Harness

## Current State
The existing `test_ink_parity_unified.sh` provides:
- 3-environment testing (deno, runts dev, runts build)
- Similarity calculation between outputs
- Symbol extraction for per-symbol diff
- Known issues tracking
- Unit test integration

## Improvements Needed

### 1. Better Output Normalization
- Handle ANSI escape codes consistently
- Normalize whitespace across environments
- Handle terminal size differences
- Handle cursor movement sequences

### 2. Enhanced Diff Reporting
- Per-symbol diff reporting
- Side-by-side comparison view
- HTML report generation option
- Failure categorization (style, layout, content)

### 3. Environment-Specific Handling
- Deno: Handle React 19 compatibility issues
- runts dev: Handle HIR runtime limitations
- runts build: Handle compilation differences

### 4. Timeout & Error Handling
- Better timeout handling for interactive apps
- Improved error message extraction
- Stack trace capture for crashes

### 5. CI/CD Integration
- JUnit XML output
- GitHub Actions annotations format
- Slack/Discord webhook notifications

## Implementation Plan

1. Create `src/test-harness.ts` for shared testing utilities
2. Enhance `test_ink_parity_unified.sh` with:
   - Better output normalization
   - Per-symbol diff reporting
   - Improved error handling
3. Add `crates/runts-ink/tests/ink_parity_harness_tests.rs`

## Status: IN PROGRESS
