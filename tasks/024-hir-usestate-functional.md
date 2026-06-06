# Task 024: Implement useState Functional Updater + useReducer in HIR Runtime

**Priority:** P0-Critical  
**Phase:** 2 — HIR Runtime Core Engine  
**ETA:** 2–3 hours  
**Depends on:** 022 (hooks module must exist)

## The Problem

Ink's `useState` supports **functional updates**:

```tsx
const [count, setCount] = useState(0);
setCount(c => c + 1);  // updater function receives previous state
```

The HIR runtime's `call_use_state` returns a `HookSetter { idx }`. When called, it blindly overwrites the slot:

```rust
Value::HookSetter { idx } => {
    let arg = arguments.first().map(|a| self.eval_expr(a)).transpose()?.unwrap_or(Value::Undefined);
    if let Some(HookSlot::State { value, .. }) = self.hook_slots.get_mut(idx) {
        *value = arg;  // BUG: ignores updater function semantics
    }
    Ok(Value::Undefined)
}
```

If `arg` is a `Value::Function`, the runtime **stores the function as the new state** instead of calling it with the previous state.

This breaks:
- `ink-counter` (the flagship example)
- Any example using `setX(prev => prev + 1)`
- Any form with derived state

## Why This Is P0

- `ink-counter` is the most important example. If it doesn't work, the demo is dead.
- EXECUTE.md: *"If HIR or HIR runtime doesn't support something to be compatible with Ink, you have to implement it."*
- Functional updates are part of the Preact/React hook contract.

## Steps

### Step 1: Fix `HookSetter` invocation

In `interpreter/hooks.rs` (or `hir_runtime.rs` before 022), change the `HookSetter` arm:

```rust
Value::HookSetter { idx } => {
    let arg = arguments
        .first()
        .map(|a| self.eval_expr(a))
        .transpose()?
        .unwrap_or(Value::Undefined);

    if let Some(HookSlot::State { value, .. }) = self.hook_slots.get_mut(idx) {
        match arg {
            Value::Function { params, body } => {
                // Functional update: call updater with previous state
                let prev = value.clone();
                let updater_args = if params.len() >= 1 {
                    vec![prev]
                } else {
                    vec![]
                };
                *value = self.call_function(&params, &body, &updater_args)?;
            }
            other => {
                // Direct value update
                *value = other;
            }
        }
    }
    Ok(Value::Undefined)
}
```

### Step 2: Add `useReducer`

Ink examples don't use `useReducer` directly, but it's the canonical way to implement complex state logic and is often used internally.

```rust
fn call_use_reducer(
    &mut self,
    arguments: &[Expr],
) -> Result<Value, RuntimeError> {
    let reducer = arguments
        .first()
        .map(|a| self.eval_expr(a))
        .transpose()?
        .unwrap_or(Value::Undefined);
    let initial = arguments
        .get(1)
        .map(|a| self.eval_expr(a))
        .transpose()?
        .unwrap_or(Value::Undefined);

    let idx = self.hook_idx;
    self.hook_idx += 1;

    if idx >= self.hook_slots.len() {
        self.hook_slots.push(HookSlot::State { value: initial.clone() });
    }

    let dispatch = Value::Function {
        params: vec!["action".to_string()],
        body: Box::new(Expr::Undefined), // placeholder — real implementation
    };

    // Return [state, dispatch]
    Ok(Value::Array(vec![
        Value::HookState { idx },
        dispatch,
    ]))
}
```

**Note:** A full `useReducer` dispatch needs to call the reducer function with `(state, action)`. For the parity goal, a simplified version that supports basic reducers is sufficient.

### Step 3: Add unit tests

```rust
#[test]
fn test_use_state_functional_updater() {
    let src = r#"
export default function App() {
  const [count, setCount] = useState(0);
  setCount(c => c + 1);
  setCount(c => c + 1);
  return <Text>{count}</Text>;
}
"#;
    let output = render_tsx(src, 80, 24).unwrap();
    assert!(output.contains("2"), "Expected count=2 after two increments, got: {}", output);
}
```

```rust
#[test]
fn test_use_state_direct_update() {
    let src = r#"
export default function App() {
  const [name, setName] = useState("Alice");
  setName("Bob");
  return <Text>{name}</Text>;
}
"#;
    let output = render_tsx(src, 80, 24).unwrap();
    assert!(output.contains("Bob"));
}
```

### Step 4: Verify against `ink-counter`

```bash
./scripts/parity.sh --env hir --examples ink-counter
```

Expected: similarity ≥ 95% with deno initial frame.

## Acceptance Criteria

- [ ] `setCount(c => c + 1)` evaluates the arrow function and passes previous state.
- [ ] Direct updates (`setName("Bob")`) still work.
- [ ] `ink-counter` initial render matches deno output.
- [ ] Unit tests exist for both functional and direct updates.
- [ ] No regression on existing `useState` tests.

## Notes

- The hook call order must remain deterministic. Functional updates happen during render, not asynchronously.
- If the updater returns `Undefined`, the state should become `Undefined` (matching JS behavior).
- Do not implement async batched updates — HIR runtime is synchronous.
