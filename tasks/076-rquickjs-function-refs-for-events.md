# Task 076: Use rquickjs Function References for Event Dispatch

## Status: 🟡 **SIGNIFICANT IMPROVEMENT — NOT STARTED**

## Goal
Replace string-based `ctx.eval()` event dispatch with direct `rquickjs::Function` calls.

## Problem

Every keyboard and mouse event builds a JavaScript string and evaluates it:

```rust
// event_loop.rs — EVERY KEYSTROKE:
let dispatch_js = format!(
    "if(typeof __tb_dispatch_key==='function'){{__tb_dispatch_key({}, {}, {}, {}, {})}}",
    serde_json::to_string(&key_str)?, ctrl, shift, alt, meta,
);
js_ctx.with(|ctx| {
    ctx.eval::<(), _>(dispatch_js.as_str())  // Parse + execute JS every keystroke!
});
```

This allocates a new string, parses it as JS, and executes it **on every single key event**. At 60fps with rapid typing or gaming input, this is wasteful.

## Fix Approach

Look up the JS dispatch functions once at startup, store them as `rquickjs::Function`, and call them directly:

```rust
// In main.rs or event_loop.rs setup:
let dispatch_key: rquickjs::Function = ctx.globals().get("__tb_dispatch_key")?;
let dispatch_mouse: rquickjs::Function = ctx.globals().get("__tb_dispatch_mouse")?;
let invoke_timers: rquickjs::Function = ctx.globals().get("__tb_invoke_timers")?;

// Later, per event — no string building, no parsing:
dispatch_key.call((key_str, ctrl, shift, alt, meta))?;
```

## Implementation Steps

1. In `setup_runtime()`, after loading `runtime.js`, extract the three dispatch functions from `ctx.globals()`.
2. Store them in a struct (e.g., `JsDispatchers`) that is passed to `run_event_loop()`.
3. Update `handle_key_event()` to call `dispatch_key.call(...)` instead of `ctx.eval(...)`.
4. Update `handle_mouse_event()` similarly.
5. Update `poll_timers()` to call `invoke_timers.call((timer_ids_json,))` instead of string eval.

## Lifetime Considerations

`rquickjs::Function` is tied to the `Context` lifetime. Since the event loop runs within the same scope as `main()`, this is straightforward — store the functions in `main()` and pass references to `run_event_loop()`.

If `Function` references become complex, wrap them in a struct that holds the `Context`:

```rust
pub struct JsDispatchers<'ctx> {
    pub dispatch_key: rquickjs::Function<'ctx>,
    pub dispatch_mouse: rquickjs::Function<'ctx>,
    pub invoke_timers: rquickjs::Function<'ctx>,
}
```

## Performance Impact

| Path | Before | After | Improvement |
|------|--------|-------|-------------|
| Key dispatch | String alloc + JS parse + eval | Direct C call | **~10x** (~0.5ms → ~0.05ms) |
| Mouse dispatch | String alloc + JS parse + eval | Direct C call | **~10x** |
| Timer batch | String alloc + JS parse + eval | Direct C call | **~5x** |

## Acceptance Criteria
- [ ] `setup_runtime()` extracts `__tb_dispatch_key`, `__tb_dispatch_mouse`, `__tb_invoke_timers` as `rquickjs::Function`
- [ ] `handle_key_event()` uses `.call()` instead of string `ctx.eval()`
- [ ] `handle_mouse_event()` uses `.call()` instead of string `ctx.eval()`
- [ ] `poll_timers()` uses `.call()` instead of string `ctx.eval()`
- [ ] All examples still respond to keyboard/mouse input correctly
- [ ] Frame budget improved in benchmarks

## Files to Modify
- `src/main.rs` — Extract and store Function refs after runtime.js load
- `src/event_loop.rs` — Accept `JsDispatchers`, use `.call()` in event handlers

## References
- rquickjs Function docs: https://docs.rs/rquickjs/latest/rquickjs/struct.Function.html
- Task 055 (Hot Path Optimization — current batching approach)
- Task 053 (Function Callbacks — superseded by 055, now revisited)
