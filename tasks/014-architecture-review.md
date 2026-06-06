# Task 014: Architecture Review - Full Project Analysis

**Date:** 2026-06-06
**Status:** Completed

## Overview

This document provides a comprehensive architecture review of the runts-ink project, identifying the key components, their relationships, and areas requiring attention for achieving 100% parity across all 3 environments.

---

## 1. Project Structure

```
runts/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── hir_runtime.rs       # HIR interpreter (3087 lines)
│   ├── transpile/           # TypeScript to Rust transpilation
│   └── runtime/             # JS runtime bindings
├── crates/
│   ├── runts-ink/           # Ink-compatible components
│   │   └── src/
│   │       ├── components.rs   # Box, Text, Spacer, etc.
│   │       ├── render.rs       # Rendering pipeline
│   │       ├── js_bridge.rs    # JS<->Rust FFI
│   │       ├── style.rs        # Style enums
│   │       ├── events.rs        # Event handling
│   │       ├── vnode.rs         # Virtual node types
│   │       └── flex_layout/     # Yoga/Taffy integration
│   ├── runts-ratatui/       # Ratatui plugin
│   ├── runts-plugin/        # Plugin trait definitions
│   └── runts-fresh/         # Fresh framework support
├── examples/
│   └── ink-*                # 88 Ink examples
└── tests/
    ├── ink_parity_tests.rs
    └── ink_parity_harness_tests.rs
```

---

## 2. Three-Environment Architecture

### Environment 1: Deno (Reference)
- **Runtime:** `deno run -A main.tsx`
- **Packages:** `npm:ink@7`, `npm:react@19`
- **Purpose:** Ground truth for expected output

### Environment 2: runts dev (HIR Runtime)
- **Command:** `runts hir-render tui/app.tsx`
- **Implementation:** `src/hir_runtime.rs` - Pure Rust interpreter
- **Features:**
  - Hook support: `useState`, `useEffect`, `useMemo`, `useCallback`
  - Context: `createContext`, `useContext`
  - JSX lowering to VNode tree
  - Static rendering (no interactive input)

### Environment 3: runts compile (Rust Codegen)
- **Command:** `runts build --plugin ratatui`
- **Implementation:** 
  - Transpiles TSX to HIR
  - `runts-ratatui` plugin generates Rust code
  - Compiles with `runts-ink` runtime
- **Features:** Full interactivity support

---

## 3. Key Components Analysis

### 3.1 HIR Runtime (`src/hir_runtime.rs`)

**Purpose:** Interprets HIR (High-level IR) directly without JavaScript engine.

**Strengths:**
- Fast startup (no JS engine)
- Works on bare embedded systems
- Hook state management built-in

**Limitations:**
- Static rendering only
- No `useInput`/`useStdin` support
- Simplified JS expression evaluation

**Key Types:**
```rust
pub enum Value {
    String(String),
    Number(f64),
    Boolean(bool),
    VNode(VNode),
    HookState { idx: usize },
    HookSetter { idx: usize },
    Function { params: Vec<String>, body: Box<hir::Expr> },
}
```

### 3.2 runts-ink Components (`crates/runts-ink/`)

**Core Components:**
- `Box` - Flexbox container with layout props
- `Text` - Styled text with color/modifiers
- `Spacer` - Empty space
- `Newline` - Line break
- `Static` - Unchanging content
- `Transform` - Text transformation

**Styling:**
- Colors: 256-color palette + RGB
- Border styles: single, double, round, etc.
- Text styles: bold, italic, dim, underline, strikethrough

### 3.3 Ratatui Plugin (`crates/runts-ratatui/`)

**Codegen Path:**
1. Parse TSX to HIR
2. Generate Rust widget code
3. Embed `runts-ink` VNode rendering

**Files:**
- `codegen.rs` - Widget code generation
- `dev_jsx/mod.rs` - JSX to JS lowering
- `dev_jsx/lower.rs` - Ink component transformation

---

## 4. Parity Test Harness

### Current Implementation

```bash
./run_parity_tests.sh [OPTIONS]
  --quick         Skip compilation step
  --strict        Treat known failures as errors
  --examples N    Specific examples to test
  --verbose       Show detailed output
  --per-symbol    Show per-symbol diff
```

### Test Flow

1. **Extract examples** - Scan `examples/ink-*`
2. **Run deno** - `deno run -A main.tsx`
3. **Run HIR** - `runts hir-render tui/app.tsx`
4. **Run compile** - `runts build && ./run`
5. **Compare outputs** - Normalize and calculate similarity
6. **Report results** - Pass/fail per example

### Similarity Calculation

```bash
similarity = (matching_lines * 2) / (total_lines_deno + total_lines_hir) * 100
```

Target: ≥60% similarity for all examples

---

## 5. Identified Issues

### 5.1 Critical Issues Fixed

1. **plugin.rs struct missing** - `RatatuiPlugin` was declared empty
2. **dev_jsx.rs deleted** - Required module was removed
3. **codegen.rs deleted** - Widget codegen was removed
4. **Lifetime errors** - `get_ratatui_state` had lifetime issues
5. **Format string bug** - `lower.rs` had malformed format!

### 5.2 Remaining Known Differences

| Feature | Deno | HIR Runtime | Compile |
|---------|------|-------------|---------|
| `useState` | ✅ | ✅ (static) | ✅ |
| `useEffect` | ✅ | ⚠️ (limited) | ✅ |
| `useInput` | ✅ | ❌ (static) | ✅ |
| `useStdin` | ✅ | ❌ (skip) | ❌ |
| `useFocus` | ✅ | ⚠️ (static) | ✅ |

---

## 6. Recommendations

### 6.1 Short-term
- [x] Fix build errors in runts-ratatui
- [x] Restore deleted files
- [ ] Run full parity tests
- [ ] Document known differences clearly

### 6.2 Medium-term
- [ ] Implement `useInput` in HIR runtime
- [ ] Improve similarity threshold handling
- [ ] Add more unit tests for edge cases

### 6.3 Long-term
- [ ] Full hook parity in HIR runtime
- [ ] WebAssembly compilation target
- [ ] Interactive demo harness

---

## 7. File Size Violations

The build.rs linter reports 39 violations:

| Category | Count | Max Allowed |
|----------|-------|-------------|
| Files >500 lines | 17 | 0 |
| Functions >40 lines | 6 | 0 |
| Functions >10 complexity | 6 | 0 |

**Note:** These are warnings, not errors. Production should address them.

---

## 8. Conclusion

The project architecture is well-designed with clear separation between:
- Transpilation layer (TypeScript → HIR)
- Runtime layer (HIR → VNode → Rendered output)
- Plugin layer (HIR → Framework-specific code)

The 3-environment approach provides excellent flexibility:
- **Deno:** Quick iteration, npm ecosystem
- **HIR:** Fast startup, embedded systems
- **Compile:** Maximum performance, single binary

**Status:** Ready for full parity testing.
