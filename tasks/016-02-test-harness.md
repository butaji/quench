# Task 016-02: Fix Parity Test Harness

**Date:** 2026-06-06
**Status:** In Progress

## Problem Statement

The test harness currently:
1. Deno examples using useInput/stdin hang indefinitely
2. Timeout is not available on macOS (no GNU timeout)
3. Interactive examples need special handling
4. Similarity calculation may not work correctly

## Required Fixes

### 1. Cross-platform timeout implementation
The harness uses `run_with_timeout` but needs to handle:
- Background processes that hang
- Interactive examples that wait for input

### 2. Interactive example detection
Examples using these patterns should be marked as interactive:
- `useInput` hook
- `useStdin` hook
- `useFocus` hook
- stdin/stdout/stderr access

### 3. Better output handling
- Pipe empty input to stdin for interactive examples
- Detect when example expects input and skip/fail gracefully

## Implementation Plan

```bash
# For deno examples that use useInput:
# Option 1: Pipe input (q to quit)
echo "q" | deno run -A main.tsx

# Option 2: Mark as interactive and skip
# Option 3: Check source code for useInput and handle specially
```

## Test Categories

| Category | Deno | HIR | Compile | Similarity |
|----------|------|-----|---------|------------|
| Static | ✓ | ✓ | ? | 100% |
| useState only | ✓ | ✓ (static) | ? | 80%+ |
| useEffect | ✓ | ✓ (once) | ? | 80%+ |
| Interactive | ✗ (needs TTY) | ✗ | ? | N/A |

## Changes Needed

1. Update `run_deno()` to handle stdin
2. Add interactive example detection
3. Improve similarity calculation
4. Add per-symbol diff reporting
