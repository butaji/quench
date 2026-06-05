# Task 009: Enhance Parity Harness for 3-Environment Testing

## Objective
Enhance the parity test harness to cover all 3 environments:
1. **deno** - Real Ink npm package execution
2. **runts dev** - HIR runtime (QuickJS/HIR interpreter)
3. **runts compile** - In-memory transpile + Rust compilation

## Current State
The current `run_parity_tests.sh` only tests deno vs HIR (D-H). The runts compile path is not included.

## Implementation Plan

### 1. Add `run_compile()` function to test Rust codegen path
- Compile the example using `runts build`
- Run the compiled binary
- Capture and normalize output

### 2. Calculate 3-way similarity scores
- Deno vs HIR (D-H)
- Deno vs Compile (D-C)
- HIR vs Compile (H-C)

### 3. Generate per-symbol diff results
- For each example, show which symbols differ
- Highlight unique symbols in each environment

### 4. Update reporting format
- Summary table with all 3 similarity scores
- Per-example diff details
- Clear pass/fail indicators

## Acceptance Criteria
- [x] Deno vs HIR testing (existing)
- [ ] Deno vs Compile testing (new)
- [ ] HIR vs Compile testing (new)
- [ ] Per-symbol diff reporting (new)
- [ ] All 88 examples tested across all 3 environments
- [ ] Clear pass/fail output
