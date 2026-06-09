# Task 054: Rust Reconciler

## Goal
Implement React-style reconciler in Rust for stateful components with useState, useEffect, and re-rendering.

## The Problem

**Current state:**
- `ink_js.rs::useState()` returns `[initial, fn(){}]` — setter is a no-op
- `ink_js.rs::useEffect()` is a no-op
- `src/js/runtime.js` provides a JS reconciler, but it's 677 lines of JS
- Without a reconciler, components can't have state that triggers re-renders

**Counter example (what users want):**
```tsx
function Counter() {
  const [count, setCount] = useState(0);
  useInput((input) => {
    if (input === ' ') setCount(c => c + 1);
  });
  return <Text>Count: {count}</Text>;
}
```

This requires:
1. State storage per component instance
2. Hook call ordering (React rules)
3. Re-render trigger when state changes
4. Effect scheduling and cleanup

## Simplified Approach (MVP)

Instead of a full reconciler with diffing, start with **naive re-rendering**:

1. **State storage**: HashMap of instance_id → hook values
2. **Re-render**: Destroy old tree, re-run component, build new tree
3. **Hook context**: Vec of hook states, indexed by call order
4. **Effects**: Run after render, compare deps

Why naive is acceptable:
- Ink trees are small (tens of nodes, not thousands)
- Yoga layout is fast (~1ms)
- ratatui render is fast (~1ms)
- Total re-render: ~2-5ms — well within 16ms for 60fps

## Architecture

### Hook Context (per component instance)
```rust
pub struct HookContext {
    hooks: Vec<HookState>,
    current_index: usize,
}

pub enum HookState {
    State { value: rquickjs::Value },
    Effect { deps: Vec<rquickjs::Value>, cleanup: Option<rquickjs::Function> },
    Ref { value: rquickjs::Value },
}
```

### Component Instance
```rust
pub struct ComponentInstance<'js> {
    id: u32,
    fn_ref: rquickjs::Function<'js>,
    props: rquickjs::Value<'js>,
    hook_ctx: HookContext,
    root_id: u32,
}
```

### Reconciler
```rust
pub struct Reconciler {
    instances: HashMap<u32, ComponentInstance>,
    dirty: Vec<u32>,
}
```

### useState with Re-render
```rust
pub fn use_state<'js>(ctx: Ctx<'js>, initial: Value<'js>) -> JsResult<Value<'js>> {
    let instance = get_current_instance();
    let idx = instance.hook_ctx.current_index;
    instance.hook_ctx.current_index += 1;
    
    if idx >= instance.hook_ctx.hooks.len() {
        // First render
        let setter = create_setter(ctx, instance.id, idx)?;
        instance.hook_ctx.hooks.push(HookState::State { value: initial });
    }
    
    let value = match &instance.hook_ctx.hooks[idx] {
        HookState::State { value } => value.clone(),
        _ => return Err(JsError::new("Hook mismatch")),
    };
    
    // Return [value, setter]
    let arr = ctx.eval::<Object, _>("[undefined, undefined]")?;
    arr.set(0, value)?;
    arr.set(1, setter)?;
    Ok(arr.into())
}
```

## Re-render Flow
1. User calls `setCount(5)`
2. Setter updates `HookState::State.value`
3. Setter calls `reconciler.schedule_rerender(instance_id)`
4. Event loop sees dirty flag
5. Event loop calls `reconciler.rerender(instance_id)`
6. Re-render:
   - `hook_ctx.current_index = 0`
   - Run component function (hooks read from storage)
   - Destroy old tree (`__ink_destroy_root`)
   - Build new tree from returned element
   - `__ink_commit()`

## Acceptance Criteria
- [ ] `useState(initial)` returns `[value, setter]` where setter triggers re-render
- [ ] `useEffect(fn, deps)` runs effect when deps change, supports cleanup
- [ ] `useRef(initial)` returns `{current: initial}` that persists across renders
- [ ] Hook rules enforced (same order, same count per component)
- [ ] Re-render destroys old tree and builds new one (naive approach)
- [ ] Effects run after render completes

## Performance Target
- Initial render: <5ms
- Re-render: <5ms (naive rebuild)
- State update to screen: <16ms (60fps)

## Dependencies
- Task 009b (ink_js.rs integration)
- Task 053 (Function callbacks for setter dispatch)

## SPEC Reference
§3 Rust ink Module; §7 Performance
