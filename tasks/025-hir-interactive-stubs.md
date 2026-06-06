# Task 025: Implement Real useInput / useApp / useStdin / useFocus Stubs in HIR Runtime

**Priority:** P1-High  
**Phase:** 2 — HIR Runtime Core Engine  
**ETA:** 3–4 hours  
**Depends on:** 022

## The Problem

The HIR runtime currently returns `Value::Undefined` or mock objects for every interactive hook:

| Hook | Current HIR Behavior | Ink Behavior |
|------|---------------------|--------------|
| `useInput(handler)` | Returns `Value::Undefined` | Calls `handler(input, key)` on every keypress |
| `useApp()` | Returns `{ exit: fn }` | Returns `{ exit, stdout, stdin, stderr }` |
| `useStdin()` | Returns `{ isRawModeSupported: false, setRawMode: fn }` | Real stdin stream |
| `useStdout()` | Returns `{ write: fn }` | Real stdout stream |
| `useStderr()` | Returns `{ write: fn }` | Real stderr stream |
| `useWindowSize()` | Returns `{ width: 80, height: 24 }` | Reads actual terminal size |
| `useFocus()` | Returns `{ isFocused: true, focus: fn }` | Manages focus state |

When JSX tries to call `useInput((input, key) => { ... })`, the hook returns `Undefined`. The JSX runtime then tries to call `undefined(...)` and silently does nothing.

This means **14 interactive examples** cannot even render their initial frame without crashing or omitting content.

## Why This Matters

- EXECUTE.md requires parity across **all 3 environments**.
- Even if HIR cannot be interactive (no event loop), it must **render the initial static frame identically** to deno.
- Many interactive examples compute layout or content inside the `useInput` callback that is also referenced in render. If the hook crashes, render breaks.

## Steps

### Step 1: `useInput` — return a no-op registration function

Ink's `useInput` is called as a hook:

```tsx
useInput((input, key) => { ... });
```

It returns `void`. In HIR runtime, we cannot process events, but we **must not crash**.

Current broken code:
```rust
fn call_use_input(&mut self, _arguments: &[Expr]) -> Result<Value, RuntimeError> {
    Ok(Value::Undefined)  // Caller expects to call this as a function? No.
}
```

Wait — actually `useInput` is called directly, not returned. The problem is that if the handler references `setCount` or other hooks, those must be resolvable in scope.

**Actual fix:** `useInput` should evaluate its first argument (the handler function), store it in a hook slot (so it's called in order), and return `Value::Undefined`.

```rust
fn call_use_input(&mut self, arguments: &[Expr]) -> Result<Value, RuntimeError> {
    let handler = arguments
        .first()
        .map(|a| self.eval_expr(a))
        .transpose()?
        .unwrap_or(Value::Undefined);

    let idx = self.hook_idx;
    self.hook_idx += 1;

    if idx >= self.hook_slots.len() {
        self.hook_slots.push(HookSlot::Effect {
            last_deps: vec![],
            body: handler,
        });
    }

    Ok(Value::Undefined)
}
```

This ensures hook count is stable and the handler is preserved.

### Step 2: `useApp` — return complete mock

```rust
fn call_use_app(&mut self, _arguments: &[Expr]) -> Result<Value, RuntimeError> {
    Ok(Value::Object({
        let mut m = std::collections::HashMap::new();
        m.insert("exit".to_string(), Value::Function {
            params: vec!["code".to_string()],
            body: Box::new(hir::Expr::Undefined),
        });
        m.insert("stdout".to_string(), Value::Object({
            let mut s = std::collections::HashMap::new();
            s.insert("write".to_string(), Value::Function {
                params: vec!["data".to_string()],
                body: Box::new(hir::Expr::Undefined),
            });
            Value::Object(s)
        }));
        m.insert("stdin".to_string(), Value::Object({
            let mut s = std::collections::HashMap::new();
            s.insert("isRawModeSupported".to_string(), Value::Boolean(false));
            s.insert("setRawMode".to_string(), Value::Function {
                params: vec!["raw".to_string()],
                body: Box::new(hir::Expr::Undefined),
            });
            Value::Object(s)
        }));
        Value::Object(m)
    }))
}
```

### Step 3: `useStdin` / `useStdout` / `useStderr` — consistent mock objects

Ensure they return the same shape as `useApp`'s sub-objects. Many examples destructure:

```tsx
const { isRawModeSupported } = useStdin();
```

The mock must have `isRawModeSupported` as a key.

### Step 4: `useWindowSize` — read actual terminal or respect env var

Currently hardcoded to `(80, 24)`. Change to:

```rust
fn call_use_window_size(&mut self, _arguments: &[Expr]) -> Result<Value, RuntimeError> {
    let (cols, rows) = if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(rows) = std::env::var("LINES") {
            (cols.parse().unwrap_or(80), rows.parse().unwrap_or(24))
        } else {
            (80, 24)
        }
    } else {
        (80, 24)
    };

    Ok(Value::Object({
        let mut m = std::collections::HashMap::new();
        m.insert("width".to_string(), Value::Number(cols as f64));
        m.insert("height".to_string(), Value::Number(rows as f64));
        Value::Object(m)
    }))
}
```

### Step 5: `useFocus` — stable mock

Already returns a mock, but verify it works with destructuring:

```tsx
const { isFocused, focus } = useFocus();
```

Mock must have both keys.

### Step 6: Run all interactive examples through HIR

```bash
./scripts/parity.sh --env hir --examples ink-counter ink-input ink-focus ink-window-size ink-stdin ink-stdout ink-stderr ink-use-app ink-key-events ink-mouse-events ink-focus-cycle ink-focus-manager ink-focus-next ink-enter-submit
```

Every example must render its initial frame without panic.

## Acceptance Criteria

- [ ] All 14 interactive examples render initial frame in HIR without crashing.
- [ ] `useInput` evaluates its handler argument and returns `Undefined`.
- [ ] `useApp` returns an object with `exit`, `stdout`, `stdin` keys.
- [ ] `useStdin` returns an object with `isRawModeSupported` and `setRawMode`.
- [ ] `useWindowSize` reads `COLUMNS`/`LINES` env vars before falling back to 80×24.
- [ ] `useFocus` returns `{ isFocused: true, focus: fn }`.

## Notes

- We are not implementing real interactivity in HIR. We are implementing **graceful degradation**.
- The parity harness must document: *"Interactive examples compared by initial static frame only."*
- If an example's render output depends on a side-effect from `useInput` (e.g. counting keypresses), that example will never reach 100% parity in HIR. This is expected. Document it.
