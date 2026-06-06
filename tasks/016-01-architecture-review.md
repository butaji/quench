# Task 016-01: Architecture Review - Complete Project Analysis

**Date:** 2026-06-06
**Status:** In Progress

## Summary

Comprehensive review of runts-ink architecture and full project structure.

---

## Project Structure Overview

```
runie-tsx/
├── src/                      # Core runts compiler
│   ├── main.rs              # CLI entry point
│   ├── cli.rs               # CLI commands
│   ├── config.rs            # Configuration
│   ├── hir_runtime.rs       # HIR interpreter (dev mode)
│   ├── commands/            # CLI commands (dev, build, etc.)
│   ├── runtime/             # Server runtime
│   ├── transpile/           # TS/TSX → Rust pipeline
│   │   ├── parser/         # TypeScript parser
│   │   ├── hir/            # High-level IR
│   │   ├── analyzer/       # Semantic analysis
│   │   ├── js_codegen/     # JS codegen (deprecated)
│   │   ├── routegen.rs     # Route generation
│   │   └── middlewaregen.rs # Middleware generation
│   └── util/                # Utilities
├── crates/
│   ├── runts-ink/          # Ink TUI components (Rust)
│   │   └── src/
│   │       ├── components.rs   # Box, Text, Newline, etc.
│   │       ├── events.rs       # InputEvent, Key, etc.
│   │       ├── flex_layout/    # Yoga/Taffy bridge
│   │       ├── js_bridge.rs    # rquickjs FFI
│   │       ├── render.rs       # VNode rendering
│   │       ├── style.rs        # Style types
│   │       └── vnode.rs        # VNode types
│   ├── runts-ratatui/      # Ratatui codegen plugin
│   │   └── src/
│   │       ├── codegen/     # Rust codegen
│   │       │   ├── app.rs   # App generation
│   │       │   ├── expr.rs  # Expression codegen
│   │       │   ├── vars.rs  # Variable codegen
│   │       │   ├── ink/     # Ink-specific codegen
│   │       │   └── ink_widget.rs # Widget generation
│   │       ├── plugin.rs   # Plugin implementation
│   │       └── plugin_test/ # Tests
│   └── runts-*/            # Other plugins
├── plugins/
│   └── crates/              # Plugin symlinks
├── examples/
│   └── ink-*/              # 88 ink examples
├── tests/                   # Integration tests
├── tasks/                   # Task tracking
└── build.rs                 # Linter rules
```

---

## Three Environment Pipeline

### 1. Deno (Reference)
- Uses `npm:ink@7` and `npm:react@19`
- Full JavaScript runtime
- Real async event loop
- Native terminal support via Ink

### 2. runts dev (HIR Runtime)
- Custom Rust interpreter for HIR
- Direct VNode generation
- QuickJS for JS evaluation (when needed)
- Hot reload via file watching
- Single-threaded synchronous execution

### 3. runts compile (Rust Codegen)
- TS/TSX → HIR via custom parser
- HIR → Rust via code generator
- Compile with cargo
- Binary output with embedded runtime

---

## Ink Components Coverage

| Component | Status | Notes |
|-----------|--------|-------|
| `Box` | ✅ Implemented | Flexbox container |
| `Text` | ✅ Implemented | Text with styles |
| `Newline` | ✅ Implemented | Line break |
| `Spacer` | ✅ Implemented | Empty space |
| `Static` | ✅ Implemented | Static content |
| `Transform` | ✅ Implemented | Transform wrapper |
| `Raw` | ❌ Missing | Raw output |
| `Measure` | ❌ Missing | Dimension measurement |
| `Border` | ⚠️ Partial | Uses Box borders |
| `Context` | ⚠️ Partial | Limited support |

---

## Ink Hooks Coverage

| Hook | Deno | HIR | Compile |
|------|------|-----|---------|
| `useState` | ✅ | ✅ (static) | ⚠️ Broken |
| `useEffect` | ✅ | ⚠️ (once) | ❌ |
| `useInput` | ✅ | ❌ | ❌ |
| `useFocus` | ✅ | ⚠️ (static) | ❌ |
| `useApp` | ✅ | ⚠️ | ❌ |
| `useStdin` | ✅ | ❌ | ❌ |
| `useStdout` | ✅ | ❌ | ❌ |
| `useStderr` | ✅ | ❌ | ❌ |
| `useContext` | ✅ | ⚠️ | ❌ |
| `useMemo` | ✅ | ⚠️ | ❌ |
| `useCallback` | ✅ | ⚠️ | ❌ |
| `useReducer` | ✅ | ❌ | ❌ |
| `useRef` | ✅ | ✅ | ⚠️ |
| `useComputed` | ✅ | ❌ | ❌ |
| `useSignal` | ✅ | ❌ | ❌ |
| `useSignalEffect` | ✅ | ❌ | ❌ |

---

## HIR Runtime Architecture

```rust
// Core types in hir_runtime.rs
pub enum Value {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    VNode(VNode),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
    Function { params, body },
    HookState { idx },
    HookSetter { idx },
}

pub struct Interpreter {
    default_export: Option<hir::FunctionDecl>,
    scope: HashMap<String, Value>,
    hook_slots: Vec<HookSlot>,
    hook_idx: usize,
    contexts: HashMap<String, Value>,
    next_ctx_id: usize,
}
```

### Hook Implementation

```rust
fn call_use_state(&mut self, initial: Value) -> (Value, Value) {
    let idx = self.hook_idx;
    self.hook_idx += 1;
    
    if idx < self.hook_slots.len() {
        if let HookSlot::State { value } = &self.hook_slots[idx] {
            return (value.clone(), Value::HookSetter { idx });
        }
    }
    
    let value = initial;
    self.hook_slots.push(HookSlot::State { value: value.clone() });
    (value, Value::HookSetter { idx })
}
```

---

## Codegen Architecture

### TypeScript → Rust Flow

```
TS/TSX → Parser → HIR → Analyzer → Codegen → Rust
```

### Ink Codegen in crates/runts-ratatui

```rust
// crates/runts-ratatui/src/codegen/ink_widget.rs
pub fn generate_ink_widget(node: &HirNode) -> TokenStream {
    match node.tag.as_str() {
        "Box" => generate_box(node),
        "Text" => generate_text(node),
        "Newline" => generate_newline(node),
        // ...
    }
}
```

---

## Test Infrastructure

### Files
- `test_parity_complete.sh` - Main test harness
- `run_parity_tests_comprehensive.sh` - Comprehensive tests
- `tests/ink_parity_tests.rs` - Unit tests for examples
- `tests/ink_comprehensive_tests.rs` - Comprehensive tests

### Test Strategy
1. Run each example in Deno
2. Run each example in runts dev (HIR)
3. Compare outputs
4. Run each example in runts compile
5. Compare outputs
6. Generate diff reports

---

## Known Gaps

### 1. Interactive Hooks
- `useInput` not implemented in HIR
- `useFocus` limited in HIR
- No stdin/stdout/stderr in HIR

### 2. Advanced Hooks
- `useEffect` only runs once
- `useMemo`, `useCallback` not fully implemented
- `useReducer` not implemented

### 3. Advanced Components
- `Raw` component missing
- `Measure` component missing
- `Context` limited support

### 4. Codegen Issues
- Hook variable scoping broken
- Some JSX patterns not supported
- Type inference incomplete

---

## Files Needing Changes

| File | Changes |
|------|---------|
| `src/hir_runtime.rs` | Implement useInput, fix hooks |
| `crates/runts-ratatui/src/codegen/vars.rs` | Fix hook scoping |
| `crates/runts-ink/src/components.rs` | Add Raw, Measure |
| `tests/ink_parity_tests.rs` | Add missing tests |
| `test_parity_complete.sh` | Update for 3-env parity |

---

## Next Steps

1. Fix parity test harness
2. Implement missing hooks in HIR
3. Fix codegen for hooks
4. Add missing components
5. Run full parity tests
6. Add unit tests
7. Commit and push
