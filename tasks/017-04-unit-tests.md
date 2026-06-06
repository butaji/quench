# Task 017-04: Add Unit Tests for Complex HIR Runtime Sections

**Date:** 2026-06-06
**Status:** Pending
**Priority:** High

## Overview

Add comprehensive unit tests for HIR runtime, covering:
- JSX evaluation
- Hook implementation
- Expression evaluation
- Error handling

## Target Modules

### 1. HIR Interpreter (`src/hir_runtime.rs`)
- Expression evaluation (all types)
- Statement execution
- Function calls
- Array/Object operations
- String methods

### 2. Hooks Implementation
- useState (getter/setter)
- useEffect (cleanup)
- useContext (provider/consumer)
- useCallback (memoization)
- useMemo (computation)
- useInput (event handling)
- useFocus (focus management)
- useStdin/useStdout/useStderr
- useWindowSize
- useApp

### 3. JSX Evaluation
- Element creation
- Fragment handling
- Props evaluation
- Children evaluation
- Conditional rendering

### 4. Style Processing
- Color parsing
- Border drawing
- Flexbox layout
- Spacing calculation

## Test Structure

```rust
// tests/hir_runtime_tests.rs
mod jsx_evaluation_tests;
mod hooks_tests;
mod expression_tests;
mod style_tests;
mod error_handling_tests;
```

## Coverage Targets

| Module | Current | Target |
|--------|---------|--------|
| hir_runtime.rs | ~70% | 95% |
| hooks | ~60% | 90% |
| jsx | ~80% | 95% |
| style | ~75% | 90% |

## Deliverables

- `tests/hir_runtime_jsx_tests.rs`
- `tests/hir_runtime_hooks_tests.rs`
- `tests/hir_runtime_expr_tests.rs`
- Updated coverage reports
