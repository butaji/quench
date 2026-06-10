# Task 013: Event Loop (Synchronous Poll)

## Status: ✅ **Implemented — uses sync `crossterm::event::poll`, not `tokio::select!`**

## Goal
Build the event loop handling crossterm events, timers, and hot reload.

## Current Implementation

The event loop in `src/event_loop.rs` uses a **synchronous** `crossterm::event::poll(Duration::from_millis(10))` loop, not `tokio::select!`:

```rust
loop {
    if let Ok(true) = crossterm::event::poll(Duration::from_millis(10)) {
        if let Ok(event) = crossterm::event::read() {
            handle_event(event)?;
        }
    }
    poll_timers()?;
    if dirty { render_tree(terminal, root_id)?; }
}
```

This works but burns CPU (wakes every 10ms even when idle). The project depends on full `tokio` but uses none of its async I/O.

## Future Improvement

See **Task 077** for migrating to `crossterm::event::EventStream` + `tokio::select!` for proper async event handling.

## Acceptance Criteria
- [x] Event loop processes crossterm keyboard events
- [x] Event loop processes crossterm mouse events
- [x] Event loop processes terminal resize events
- [x] Event loop polls timers on each iteration
- [x] Event loop renders when tree is dirty
- [ ] `tokio::select!` with `EventStream` (deferred to Task 077)
- [ ] Unit test: inject synthetic events via channels (deferred)

## Dependencies
- Task 001

## SPEC Reference
§5 Event Loop (Rust)
