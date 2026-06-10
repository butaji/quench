# Task 086: Fix Silent Error Swallowing in load_user_code

## Status: 🔴 **CRITICAL BUG — NOT STARTED**

## Goal
Stop swallowing JS errors silently in `load_user_code()`; distinguish caught JS exceptions from uncaught errors.

## Problem

`src/main.rs::load_user_code()` recently added `ctx.catch()` which catches **everything** — syntax errors, runtime exceptions, module resolution failures — and gives the user zero feedback:

```rust
// src/main.rs ~line 117-138
ctx.with(|ctx| {
    if let Err(_e) = ctx.eval::<(), _>(&*code) {
        let _ = ctx.catch();
        // Note: We intentionally don't log the error since JS caught it
    }
});
```

The comment says "JS caught it" but `ctx.catch()` catches **all** pending exceptions, not just ones that were caught by JS `try-catch`. A completely broken script (missing closing brace, undefined variable, etc.) silently does nothing. The user sees a blank terminal.

## Fix Approach

Distinguish between:
1. **JS caught exception** — JS code had `try { ... } catch { ... }` that handled it. These are normal.
2. **Uncaught exception** — Syntax error, runtime error, or exception not caught by JS. These must be surfaced.

```rust
ctx.with(|ctx| {
    match ctx.eval::<(), _>(&*code) {
        Ok(()) => {}
        Err(e) => {
            // Check if there's a pending exception that JS didn't catch
            let pending = ctx.catch();
            if !pending.is_undefined() && !pending.is_null() {
                let msg = pending.to_string();
                eprintln!("JavaScript error: {}", msg);
                std::process::exit(1);
            }
            // If no pending exception, JS caught it internally — OK
        }
    }
});
```

Alternatively, remove `catch()` entirely and let uncaught exceptions propagate up as `rquickjs::Error`, which `main()` can format nicely.

## Acceptance Criteria
- [ ] Syntax errors in user scripts produce a clear stderr message and exit non-zero
- [ ] Runtime exceptions (undefined function, etc.) produce a clear stderr message
- [ ] Scripts that use JS `try-catch` internally still work normally
- [ ] Integration test: malformed JS script produces stderr output and exit code ≠ 0

## Related Tasks
- **Task 084** — General error-swallowing pattern across the codebase (tracing::error! swallowed)
- **Task 092** — `setup_runtime()` also swallows errors by returning `Ok(())` regardless

## Files to Modify
- `src/main.rs` — `load_user_code()` error handling

## References
- rquickjs Exception docs: https://docs.rs/rquickjs/latest/rquickjs/struct.Ctx.html
