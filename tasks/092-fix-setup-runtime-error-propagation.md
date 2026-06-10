# Task 092: Fix setup_runtime Errors Not Propagated

## Status: 🟡 **MODERATE — NOT STARTED**

## Goal
Propagate `setup_runtime()` errors instead of swallowing them with `tracing::warn!`.

## Problem

`src/main.rs::setup_runtime()` logs errors but returns `Ok(())` regardless:

```rust
fn setup_runtime(ctx: &rquickjs::Context) -> Result<()> {
    ctx.with(|ctx| {
        if let Err(e) = ink_js::register(ctx) {
            tracing::warn!("ink_js::register error: {:?}", e);  // Warn, not error
        }
    });

    ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(runtime_js) {
            tracing::error!("Runtime load error: {:?}", e);  // Error logged but OK returned
        }
    });

    ctx.with(|ctx| {
        if let Err(e) = ctx.eval::<(), _>(config_js.as_str()) {
            tracing::warn!("Bridge config injection error: {:?}", e);
        }
    });

    Ok(())  // ALWAYS returns Ok
}
```

If `runtime.js` fails to load (corrupted binary, missing file, syntax error), the app continues with no reconciler, no hooks, no components. User gets a blank terminal.

## Fix

```rust
fn setup_runtime(ctx: &rquickjs::Context) -> Result<()> {
    ctx.with(|ctx| {
        ink_js::register(ctx)
            .map_err(|e| anyhow::anyhow!("Failed to register Ink constants: {:?}", e))
    })?;

    let runtime_js = include_str!("runtime.js");
    ctx.with(|ctx| {
        ctx.eval::<(), _>(runtime_js)
            .map_err(|e| anyhow::anyhow!("Failed to load runtime.js: {:?}", e))
    })?;

    let config_js = BridgeConfig::default().to_js_injection();
    ctx.with(|ctx| {
        ctx.eval::<(), _>(config_js.as_str())
            .map_err(|e| anyhow::anyhow!("Failed to inject bridge config: {:?}", e))
    })?;

    Ok(())
}
```

## Acceptance Criteria
- [ ] `setup_runtime()` returns `Err` if `runtime.js` fails to load
- [ ] `setup_runtime()` returns `Err` if `ink_js::register` fails
- [ ] Main function exits with clear error message if setup fails
- [ ] `cargo test` passes

## Related Tasks
- **Task 084** — General JS error-swallowing pattern
- **Task 086** — `load_user_code()` swallows ALL JS errors with `ctx.catch()`

## Files to Modify
- `src/main.rs` — `setup_runtime()` error handling

## References
- anyhow error propagation: https://docs.rs/anyhow/latest/anyhow/
