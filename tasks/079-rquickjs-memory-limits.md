# Task 079: Add rquickjs Memory Limits and Sandbox

## Status: 🟡 **SIGNIFICANT IMPROVEMENT — NOT STARTED**

## Goal
Set memory and stack limits on the QuickJS runtime to prevent malicious or buggy scripts from exhausting system resources.

## Problem

The QuickJS runtime is created with no limits:

```rust
let runtime = rquickjs::Runtime::new()?;
```

A buggy `while(true) { arr.push(new Object()) }` or an infinite recursion will:
- Exhaust system RAM (no memory limit)
- Stack overflow the native thread (no stack limit)
- Potentially crash the entire TuiBridge process

Since TuiBridge runs arbitrary user code (TSX files), this is a security and reliability concern.

## Fix Approach

Set conservative limits at runtime creation:

```rust
let runtime = rquickjs::Runtime::new()?;
runtime.set_memory_limit(128 * 1024 * 1024);  // 128 MB
runtime.set_max_stack_size(1024 * 1024);      // 1 MB stack
```

Also consider:
- **Execution timeout**: For non-interactive scripts, set a max execution time
- **Module loader restrictions**: Only allow loading from trusted paths

```rust
// Optional: execution time limit for non-interactive mode
if !cli_args.interactive {
    runtime.set_interrupt_handler(|| {
        // Return true to interrupt after N operations
        false
    });
}
```

## Error Handling

When limits are exceeded, rquickjs throws an exception. TuiBridge should catch this and surface a helpful error:

```rust
ctx.eval(code).map_err(|e| {
    tracing::error!("Script exceeded memory/stack limit: {:?}", e);
    eprintln!("Error: Your script exceeded the 128 MB memory limit.");
    std::process::exit(1);
})?;
```

## Acceptance Criteria
- [ ] `Runtime::set_memory_limit(128 * 1024 * 1024)` called in `main.rs`
- [ ] `Runtime::set_max_stack_size(1024 * 1024)` called in `main.rs`
- [ ] Memory limit exceeded produces a clear user-facing error (not a panic)
- [ ] Stack overflow produces a clear user-facing error
- [ ] Add test: script that allocates 200MB array fails gracefully
- [ ] Add test: infinitely recursive function fails gracefully
- [ ] Binary size impact measured (rquickjs limit APIs are tiny)

## Files to Modify
- `src/main.rs` — Add limit calls after `Runtime::new()`

## References
- rquickjs Runtime docs: https://docs.rs/rquickjs/latest/rquickjs/struct.Runtime.html
- QuickJS memory limits: https://bellard.org/quickjs/quickjs.html#Runtime
