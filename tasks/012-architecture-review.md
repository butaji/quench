# Architecture Review: runts Ink Parity

**Date:** 2026-06-05
**Status:** In Progress

## Executive Summary

This document provides a comprehensive architecture review of the runts Ink integration, covering the three execution environments (deno, runts dev/HIR, runts build), the test harness, and the current state of example coverage.

## Architecture Overview

### Three Execution Environments

```
┌─────────────────────────────────────────────────────────────────┐
│                    INK APPLICATION (.tsx)                        │
└─────────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌───────────────┐    ┌─────────────────┐    ┌─────────────────────┐
│     deno      │    │   runts dev     │    │   runts build       │
│  (npm:ink@7)  │    │   (HIR runtime) │    │   (Rust codegen)    │
├───────────────┤    ├─────────────────┤    ├─────────────────────┤
│ QuickJS       │    │ QuickJS +      │    │ TypeScript Parser   │
│ + React 19    │    │ HIR Interpreter│    │ → HIR → Rust        │
│ + Ink npm     │    │ + Ink compat   │    │ → ratatui binary    │
└───────────────┘    └─────────────────┘    └─────────────────────┘
```

### Key Components

1. **runts-core** (`src/`)
   - Parser: oxc_parser for TypeScript/TSX
   - HIR: Typed AST with preserved type information
   - Codegen: Quote-based Rust code generation
   - CLI: Commands for dev, build, transpile

2. **runts-ink** (`crates/runts-ink/`)
   - Components: Box, Text, Newline, Spacer, Static, Transform
   - Layout: Yoga flexbox layout engine
   - Events: Input, Mouse, Resize, Focus handling
   - Render: Ratatui-based terminal rendering

3. **runts-ratatui** (`crates/runts-ratatui/`)
   - Plugin for building Ink apps as standalone binaries

### Data Flow

```
TypeScript/TSX Source
        │
        ▼
┌───────────────────┐
│ oxc_parser        │ → Typed AST
└───────────────────┘
        │
        ▼
┌───────────────────┐
│ Type-Directed     │ → HIR with preserved types
│ Lowering          │
└───────────────────┘
        │
        ├──────────────────────┐
        ▼                      ▼
┌───────────────┐    ┌───────────────────┐
│ HIR Runtime   │    │ Rust Codegen      │
│ (QuickJS)     │    │ (quote!)          │
└───────────────┘    └───────────────────┘
        │                      │
        ▼                      ▼
┌───────────────┐    ┌───────────────────┐
│ Terminal      │    │ Compiled Binary   │
│ Output        │    │ (ratatui)         │
└───────────────┘    └───────────────────┘
```

## Current State Analysis

### Working Components

1. ✅ **HIR Runtime** (`runts hir-render`)
   - Successfully renders TSX to terminal output
   - Supports Box, Text, Newline components
   - Handles flexbox layout calculations

2. ✅ **Build Pipeline** (`runts build --plugin ratatui`)
   - Transpiles TSX to Rust
   - Compiles to standalone binary
   - Produces working ratatui application

3. ✅ **Ink Components** (in runts-ink)
   - Box, Text, Newline, Spacer, Static
   - Styling: colors, bold, italic, dim
   - Flexbox: direction, justify, align, gap

4. ✅ **Test Harness** (`run_parity_tests.sh`)
   - Tests 88 examples
   - Compares deno vs HIR output
   - Generates similarity scores

### Known Limitations

1. ⚠️ **Hooks Support**
   - `useInput` - Partially supported (HIR runtime has limited support)
   - `useApp` - Limited to exit signal
   - `useFocus` - Not implemented
   - `useStdin` - Not implemented

2. ⚠️ **Advanced Features**
   - Animations - Static rendering only
   - Custom rendering - Limited support
   - Complex context patterns - Basic support only

3. ⚠️ **Some Examples May Fail**
   - Examples using unsupported hooks
   - Examples with complex interactions
   - Examples with custom render patterns

## Example Coverage

### 88 Examples in `examples/ink-*`

| Category | Count | Status |
|----------|-------|--------|
| Layout (flex, box, gaps) | 15 | ✅ Working |
| Text (styling, colors) | 12 | ✅ Working |
| Forms (input, select) | 8 | ⚠️ Partial |
| Hooks (useInput, useApp) | 10 | ⚠️ Partial |
| Styling (borders, background) | 12 | ✅ Working |
| Focus/Keyboard | 8 | ⚠️ Partial |
| Animations | 3 | ⚠️ Static only |
| Advanced patterns | 20 | ⚠️ Mixed |

## Parity Test Harness

### Current Implementation

```bash
./run_parity_tests.sh [OPTIONS]
  --quick         Skip compilation
  --strict        Treat known failures as failures
  --examples N    Specific examples
  --list          List all examples
  --dry-run       Show what would be tested
  --per-symbol    Show per-symbol diffs
  --output-dir D  Save results to directory
```

### Test Process

1. **Extract** - Get examples from `examples/ink-*`
2. **Run Deno** - Execute with `deno run -A`
3. **Run HIR** - Execute with `runts hir-render`
4. **Run Compile** - Build and execute binary
5. **Compare** - Normalize and calculate similarity
6. **Report** - Generate summary and diffs

### Similarity Calculation

```
similarity = (matching_lines * 100) / max(total_lines_1, total_lines_2)
```

A result of ≥60% is considered passing.

## Unit Test Coverage

### runts-ink Tests (1103 total)

| Test File | Count | Coverage |
|-----------|-------|----------|
| ink_parity_tests.rs | ~100 | Example parity |
| ink_components_tests.rs | 24 | Box, Text |
| ink_style_tests.rs | 20 | Styling |
| ink_events_tests.rs | ~50 | Input, Focus |
| ink_flex_layout_tests.rs | ~30 | Layout |
| ink_parity_harness_tests.rs | ~200 | Test harness |
| ink_harness_*.rs | ~400 | Harness internals |

## Recommendations

### High Priority

1. **Fix unsupported hooks** - Implement useFocus, useStdin
2. **Improve parity threshold** - Increase from 60% to 80%
3. **Add more unit tests** - Cover edge cases in HIR runtime

### Medium Priority

1. **Enhance diff output** - Per-symbol highlighting
2. **Add performance benchmarks** - Compare cold-start times
3. **Improve error messages** - Better failure diagnostics

### Low Priority

1. **Add more examples** - Cover edge cases
2. **Optimize binary size** - Strip unused dependencies
3. **Add benchmarks** - Track performance over time

## Files and Locations

```
runts-tsx/
├── run_parity_tests.sh           # Main test harness
├── test_ink_parity_*.sh         # Legacy test scripts
├── examples/ink-*                # 88 Ink examples
│   ├── ink-counter/
│   │   ├── main.tsx              # Entry point
│   │   ├── tui/app.tsx           # Component
│   │   ├── deno.json             # Deno config
│   │   └── runts.config.json     # runts config
│   └── ...
├── crates/
│   ├── runts-ink/                # Ink components
│   │   ├── src/components.rs     # Box, Text, etc.
│   │   ├── src/events.rs         # Event types
│   │   ├── src/render.rs         # Ratatui render
│   │   └── tests/                # Unit tests
│   ├── runts-ratatui/            # ratatui plugin
│   └── runts-lib/                # Shared runtime
├── src/
│   ├── hir_runtime.rs            # HIR interpreter
│   ├── cli.rs                    # CLI commands
│   └── transpile/                # TS→HIR→Rust
└── tests/
    ├── ink_parity_*.rs           # Parity tests
    └── e2e_*.rs                  # End-to-end tests
```
