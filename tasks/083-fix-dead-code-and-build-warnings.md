# Task 083: Fix Dead Code and Build Warnings

## Status: 🟠 **POLISH — NOT STARTED**

## Goal
Clean up unreachable code, unused `#[allow]` attributes, and `build.rs` clippy warnings.

## Issues

### 1. Dead CLI Match Arm

`src/cli.rs:70-75` and `src/cli.rs:95-97` both have identical match arms:

```rust
arg if !arg.starts_with('-') && result.script.is_none() => {
    // Compiles TSX...
    i += 1;
}
// ...
arg if !arg.starts_with('-') && result.script.is_none() => {
    result.script = Some(arg.to_string());
    i += 1;
}
```

The second arm is **unreachable** — identical guard, identical pattern, later position. The TSX arm always catches non-flag args first. Remove the dead arm.

### 2. Unused `#[allow]` Attributes

`src/event_loop.rs` has unnecessary allows:

```rust
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
fn handle_mouse_event(mouse: crossterm::event::MouseEvent, js_ctx: &rquickjs::Context) -> bool {
```

`handle_mouse_event` is ~40 lines and not complex. Remove the attributes and let clippy do its job. Also audit other `#[allow]`s for relevance.

### 3. `build.rs` Clippy Warnings

```
warning: implicit saturating sub
warning: needless range loop
```

Fixes:
- Replace `x - y` with `x.saturating_sub(y)` where underflow is possible
- Replace `for i in start..func_line { lines[i] }` with `lines.iter().take(func_line).skip(start)`

## Acceptance Criteria
- [ ] Dead CLI match arm in `cli.rs` removed
- [ ] `handle_mouse_event` allow attributes removed (and any other cargo-culted allows)
- [ ] `build.rs` passes `cargo clippy` with zero warnings
- [ ] `cargo clippy` on the entire crate shows zero warnings
- [ ] `cargo test` passes

## Files to Modify
- `src/cli.rs` — Remove dead match arm
- `src/event_loop.rs` — Remove unnecessary `#[allow]` attributes
- `build.rs` — Fix implicit saturating sub and needless range loop

## References
- Task 058 (Linter Enforcement)
