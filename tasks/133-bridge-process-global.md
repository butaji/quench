# Task 133: Add `process` Global to rquickjs Bridge

**Priority:** P0-Critical  
**Phase:** 12 — Real-World Validation  
**Depends on:** 132

## Problem

The `../tui1` example uses `process.on("SIGINT", ...)`, `process.exit(0)`, `process.stdin`, `process.stdout`, and `process.env`. These are Node.js globals that do **not exist** in rquickjs by default.

Current behavior: `process` is `undefined` → any access throws `ReferenceError: process is not defined`.

## Features to Expose

| Property | Type | Used By |
|----------|------|---------|
| `process.exit(code)` | `fn(number)` | `process.exit(0)` on SIGINT |
| `process.on(event, cb)` | `fn(string, fn)` | `process.on("SIGINT", ...)` |
| `process.stdin` | `{ isTTY?: bool, setRawMode?: fn, resume?: fn }` | Readline setup |
| `process.stdout` | `{ rows?: number, columns?: number }` | Terminal size |
| `process.env.NODE_ENV` | `string` | Build-time conditional |

## Bridge Implementation

```rust
// In js_bridge.rs or a new js_bridge/process.rs module:
let process = Object::new(ctx.clone());
process.set("exit", Function::new(...))?;
process.set("env", Object::new(...))?;
// ...
globals.set("process", process)?;
```

## Acceptance Criteria

- [ ] `process` global is available in rquickjs context
- [ ] `process.exit(0)` terminates the dev loop gracefully
- [ ] `process.env.NODE_ENV === "production"` works (for bundle compat)
- [ ] `process.stdout.rows` / `process.stdout.columns` reflect terminal size
- [ ] `process.stdin.isTTY` returns `true` when TTY is available
- [ ] `../tui1` example no longer throws `ReferenceError: process is not defined`
