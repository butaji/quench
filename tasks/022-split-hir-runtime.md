# Task 022: Split hir_runtime.rs into a Proper Interpreter Crate

**Priority:** P0-Critical  
**Phase:** 1 — Structural Integrity  
**ETA:** 3–4 hours  
**Depends on:** 020, 021

## The Problem

`src/hir_runtime.rs` is **3,087 lines** containing:

- HIR expression evaluator (`eval_expr` — 356 lines)
- JSX→Ink component mapper (`eval_jsx` — 266 lines)
- CSS property applier (`apply_box_prop` — 309 lines)
- Hook runtime (`useState`, `useEffect`, `useMemo`, `useCallback`, `useContext` — scattered)
- JavaScript stdlib polyfills (`.map`, `.filter`, `.reduce`, `.slice`, `.trim`, `.toUpperCase`, etc. — 400+ lines)
- Color parser (`parse_color` — 30 lines)
- Value→string converters (`value_to_string`, `array_to_string`, `vnode_to_string` — 50 lines)
- **70 inline integration tests** (~1,400 lines)

This is not a file. It is a **crate pretending to be a file**.

## Why This Is P0

- EXECUTE.md: *"If HIR or HIR runtime doesn't support something to be compatible with Ink, you have to implement it."*
- We need to add `useReducer`, proper `useInput`, `useApp`, `useStdin`, `useFocus`, `Static`, `Transform`, `measure`, `table`, `progress-bar`, etc.
- Adding them to a 3,087-line file guarantees bugs and review impossibility.

## Target Architecture

```
src/interpreter/
├── mod.rs          # Interpreter struct, top-level `run()`
├── expr.rs         # eval_expr + all expression types
├── jsx.rs          # eval_jsx, eval_jsx_children
├── hooks.rs        # useState, useEffect, useMemo, useCallback, useContext,
│                   # useInput, useApp, useStdin, useStdout, useStderr,
│                   # useWindowSize, useFocus
├── stdlib.rs       # call_string_method, call_array_method
├── ink_props.rs    # apply_box_prop, apply_text_prop, parse_color
├── value.rs        # Value enum, value_to_string, vnode_to_string, From/Into
└── tests/
    ├── mod.rs
    ├── static_examples.rs   # One test per static example
    ├── layout_tests.rs      # Flex, border, padding, margin
    ├── hook_tests.rs        # useState, useEffect, useContext
    └── stdlib_tests.rs      # String/array polyfills
```

## Steps

### Step 1: Create `src/interpreter/mod.rs`

Move:
- `Interpreter` struct definition
- `new()`, `run()`, `eval_top_level()`, `eval_function_body()`
- `resolve_hook_value()`
- `bind_pattern()`
- `call_function()`

### Step 2: Create `src/interpreter/expr.rs`

Move `eval_expr` and split it:

```rust
impl Interpreter {
    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Number(n) => Ok(Value::Number(*n)),
            // ... primitives
            Expr::Member { .. } => self.eval_member(expr),
            Expr::Call { .. } => self.eval_call(expr),
            Expr::JSX(jsx) => self.eval_jsx(jsx), // delegates to jsx.rs
            Expr::Bin { .. } => self.eval_binary(expr),
            Expr::Logical { .. } => self.eval_logical(expr),
            Expr::Cond { .. } => self.eval_conditional(expr),
            // ... etc
        }
    }
}
```

Each `eval_*` helper must be ≤ 40 lines. If not, split further (e.g. `eval_binary_add`, `eval_binary_cmp`).

### Step 3: Create `src/interpreter/jsx.rs`

Move `eval_jsx` and `eval_jsx_children`. Extract each tag handler:

```rust
fn handle_box(&mut self, props: Vec<(String, Value)>, children: Vec<Value>) -> Result<Value, RuntimeError>;
fn handle_text(&mut self, props: Vec<(String, Value)>, children: Vec<Value>) -> Result<Value, RuntimeError>;
fn handle_static(&mut self, props: Vec<(String, Value)>, children: Vec<Value>) -> Result<Value, RuntimeError>;
fn handle_transform(&mut self, props: Vec<(String, Value)>, children: Vec<Value>) -> Result<Value, RuntimeError>;
fn handle_user_component(&mut self, name: &str, props: Vec<(String, Value)>, children: Vec<Value>) -> Result<Value, RuntimeError>;
```

### Step 4: Create `src/interpreter/hooks.rs`

Move all `call_use_*` methods. Add the missing `useReducer`.

### Step 5: Create `src/interpreter/stdlib.rs`

Move `call_string_method` and `call_array_method`. Add unit tests for EVERY method.

### Step 6: Create `src/interpreter/ink_props.rs`

Move `apply_box_prop`, `apply_text_prop`, `parse_color`.

Refactor `apply_box_prop` into a dispatch table:

```rust
fn apply_box_prop(b: &mut InkBox, key: &str, val: &Value) {
    match key {
        "flexDirection" => set_enum_prop!(b, flex_direction, val, FlexDirection),
        "padding" => set_uniform_padding!(b, val),
        // ... generated from a table
        _ => {}
    }
}
```

### Step 7: Create `src/interpreter/value.rs`

Move `Value` enum, `PartialEq`, `value_to_string`, `array_to_string`, `vnode_to_string`, `From<Value> for VNode`.

### Step 8: Move tests

Move ALL `#[cfg(test)]` blocks from `hir_runtime.rs` into:
- `tests/interpreter_static.rs` — reads `examples/*/tui/app.tsx`, runs `render_tsx`
- `tests/interpreter_hooks.rs` — unit tests for hook behavior
- `tests/interpreter_stdlib.rs` — unit tests for string/array methods

### Step 9: Update `src/lib.rs` or `src/main.rs`

Replace `mod hir_runtime;` with `mod interpreter;` and update `use` statements.

## Acceptance Criteria

- [ ] No file in `src/` or `crates/` exceeds 500 lines.
- [ ] No function exceeds 40 lines.
- [ ] `cargo test` passes with all existing tests.
- [ ] `cargo build` passes with linter enabled.
- [ ] `render_tsx` public API is preserved (same signature).

## Notes

- This is a **move-only** task. Do not change semantics.
- If a function is 41 lines, extract one line into a helper. Do not negotiate with the linter.
- Use `git mv` semantics: preserve history by copying then deleting in the same commit.
