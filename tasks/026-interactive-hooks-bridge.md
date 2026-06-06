# Task 026: Implement Interactive Hooks in js_bridge.rs + Event Loop

**Priority:** P1-High  
**Phase:** 1 — rquickjs Dev Engine  
**ETA:** 6–8 hours  
**Depends on:** 025

## The Problem

Interactive examples (counter, input, focus, key events) need:
- `useInput(handler)` — crossterm key events → JS callback
- `useApp()` — exit lifecycle
- `useStdin()` / `useStdout()` / `useStderr()` — stream access
- `useWindowSize()` — terminal resize
- `useFocus()` / `useFocusManager()` — focus state
- `useCursor()` — cursor position
- `useAnimation()` — timed callbacks

Currently none are wired through the bridge.

## Steps

### Step 1: Design hook state machine

Rust side maintains:
```rust
struct HookState {
    input_handlers: Vec<JsFunction>,
    app_exit_tx: Option<oneshot::Sender<()>>,
    focus_ids: Vec<FocusId>,
    active_focus: usize,
    window_size: (u16, u16),
    cursor_pos: Option<(u16, u16)>,
    animations: Vec<AnimationHandle>,
}
```

### Step 2: Implement useInput

```rust
fn use_input(ctx: Ctx, handler: Function) -> JsResult<()> {
    let bridge = ctx.userdata::<InkBridge>()?;
    bridge.state.input_handlers.push(handler);
    Ok(())
}
```

In event loop:
```rust
Event::Key(key) => {
    let ink_key = Key::from_crossterm(key);
    for handler in &bridge.state.input_handlers {
        handler.call((input_str, ink_key))?;
    }
}
```

### Step 3: Implement useApp

```rust
fn use_app(ctx: Ctx) -> JsResult<Object> {
    let bridge = ctx.userdata::<InkBridge>()?;
    let obj = Object::new(ctx)?;
    let tx = bridge.state.app_exit_tx.clone();
    obj.set("exit", Function::new(ctx, move |code: i32| {
        if let Some(tx) = tx.take() { let _ = tx.send(()); }
    })?)?;
    Ok(obj)
}
```

### Step 4: Implement remaining hooks

Follow the same pattern for each hook. See `docs/INK-ARCHITECTURE.md` for detailed mappings.

### Step 5: Verify interactive examples

```bash
./scripts/parity.sh --env rq --examples ink-counter ink-input ink-focus ink-key-events
```

## Acceptance Criteria

- [ ] `ink-counter` responds to up/down arrows and q in `runts dev`.
- [ ] `ink-input` accepts keyboard input.
- [ ] `ink-focus` cycles focus with Tab.
- [ ] All 14 interactive examples work.
