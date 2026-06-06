# Task 017-03: Enhance Parity Test Harness - Full 3-Environment Support

**Date:** 2026-06-06
**Status:** In Progress
**Priority:** Critical

## Overview

Create a unified test harness that tests all 3 environments (deno, runts dev, runts compile) with:
- Per-symbol diff results
- Detailed failure analysis
- Cross-platform timeout support
- Comprehensive reporting

## Features Required

### 1. Environment Testing
- **deno**: Reference TypeScript runtime with npm:ink@5
- **runts dev**: HIR runtime (QuickJS/HIR interpreter with hot-reload)
- **runts compile**: In-memory transpile + Rust compilation

### 2. Output Analysis
- ANSI color stripping
- Whitespace normalization
- Empty line removal
- Duplicate line filtering
- Symbol extraction

### 3. Similarity Calculation
- Line-by-line comparison
- Weighted similarity scoring
- Per-symbol diff generation
- Character-level diff (optional)

### 4. Failure Categorization
- Empty output detection
- Error message parsing
- Content mismatch analysis
- Interactive vs static classification

### 5. Reporting
- Summary table
- Per-example pass/fail
- Diff files for failures
- Symbol extraction for analysis

## Implementation

### Script: test_parity_v7.sh

```bash
#!/bin/bash
# Full 3-environment parity test harness
# - Per-symbol diff analysis
# - Comprehensive reporting
# - Cross-platform timeout
# - Unit test coverage integration
```

### Key Functions

1. `check_deps()` - Verify deno and runts are available
2. `run_deno()` - Execute with deno runtime
3. `run_hir()` - Execute with HIR runtime
4. `run_compile()` - Build and execute with Rust
5. `normalize_output()` - Clean output for comparison
6. `calc_similarity()` - Calculate similarity score
7. `generate_diff()` - Create detailed diff files
8. `categorize_failure()` - Classify failures

## Success Criteria

- All 89 examples tested in all 3 environments
- Similarity score ≥60% for each pair
- Detailed diff files generated for failures
- Summary report with pass/fail breakdown
- Per-symbol analysis available

## Files to Create/Modify

1. `test_parity_v7.sh` - Main test harness
2. `tests/ink_parity_v7_tests.rs` - Rust unit tests
3. Update `tasks/index.json` with results
