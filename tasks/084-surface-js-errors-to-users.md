# Task 084: Surface JS Errors to Users

## Status: 🟠 **POLISH — NOT STARTED**

## Goal
Print user-friendly error messages when user JS code fails to load, parse, or execute, instead of silently logging with `tracing::error!`.

## Problem

Throughout `src/main.rs`, JS errors are swallowed:

```rust
// main.rs — user script error
ctx.with(|ctx| {
    if let Err(e) = ctx.eval::<(), _>(&*code) {
        tracing::error!("Script error: {:?}", e);  // User never sees this!
    }
});

// main.rs — runtime.js error
ctx.with(|ctx| {
    if let Err(e) = ctx.eval::<(), _>(runtime_js) {
        tracing::error!("Runtime load error: {:?}", e);  // Silent failure!
    }
});

// event_loop.rs — key dispatch error
if let Err(e) = ctx.eval::<(), _>(dispatch_js.as_str()) {
    tracing::warn!("Key dispatch error: {:?}", e);  // Silent!
}
```

If a user's script has a syntax error, they just see a blank terminal (or no output at all) with no indication of what went wrong. They must set `RUST_LOG=error` to see the problem.

## Fix Approach

1. **Script load errors** — Print to stderr and exit with non-zero code:

```rust
ctx.with(|ctx| {
    match ctx.eval::<(), _>(&*code) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error in {}: {:?}", script_path, e);
            std::process::exit(1);  // Or better: return Err(...)
        }
    }
});
```

2. **Runtime.js errors** — These are internal bugs, but should still be fatal:

```rust
ctx.with(|ctx| {
    ctx.eval::<(), _>(runtime_js)
        .map_err(|e| anyhow::anyhow!("Failed to load runtime.js: {:?}", e))?;
})?;
```

3. **Event dispatch errors** — Should be non-fatal but visible in debug mode:

```rust
if let Err(e) = dispatch_key.call((key_str, ctrl, shift, alt, meta)) {
    tracing::debug!("Key dispatch error: {:?}", e);
}
```

4. **Add a user-facing error formatter** that translates rquickjs exceptions into familiar JS stack traces:

```rust
fn format_js_error(e: rquickjs::Error) -> String {
    match e {
        rquickjs::Error::Exception { message, stack, .. } => {
            format!("JavaScript Error: {}\n{}", message, stack.unwrap_or_default())
        }
        _ => format!("Runtime error: {:?}", e),
    }
}
```

## Acceptance Criteria
- [ ] JS syntax errors in user scripts print a clear message to stderr and exit non-zero
- [ ] JS runtime exceptions include file name and line number when available
- [ ] `runtime.js` load failures are fatal with a clear error
- [ ] Event dispatch errors are non-fatal but logged at debug level (not warn)
- [ ] Integration test: malformed JS script produces stderr output and exit code ≠ 0
- [ ] `cargo test` passes

## Files to Modify
- `src/main.rs` — Propagate `ctx.eval` errors instead of swallowing
- `src/event_loop.rs` — Reduce event dispatch error to debug level

## References
- rquickjs Error docs: https://docs.rs/rquickjs/latest/rquickjs/enum.Error.html
