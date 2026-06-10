# Task 077: Async Event Loop with crossterm EventStream

## Status: 🟡 **SIGNIFICANT IMPROVEMENT — NOT STARTED**

## Goal
Replace the synchronous `crossterm::event::poll(Duration::from_millis(10))` loop with an async `EventStream` integrated into `tokio::select!`.

## Problem

The current event loop in `src/event_loop.rs` uses a blocking poll with a 10ms timeout:

```rust
loop {
    if let Ok(true) = crossterm::event::poll(Duration::from_millis(10)) {
        if let Ok(event) = crossterm::event::read() {
            handle_event(event)?;
        }
    }
    poll_timers()?;
    if dirty { render_tree(...)?; }
}
```

Issues:
- **Burns CPU**: Even when idle, the thread wakes every 10ms to check for events
- **Wastes tokio**: The project depends on full `tokio` but uses none of its async I/O
- **No async integration**: File watching (hot reload) and timers can't use tokio's efficient scheduling
- **Inaccurate timer resolution**: Timers are checked only every 10ms, not when they actually expire

## Fix Approach

Use `crossterm::event::EventStream` (from the `event-stream` feature, already enabled) with `tokio::select!`:

```rust
use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::time::{interval, sleep};

pub async fn run_event_loop_async(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    js_ctx: &rquickjs::Context,
) -> Result<()> {
    let mut reader = EventStream::new();
    let mut timer_interval = interval(Duration::from_millis(1));

    loop {
        tokio::select! {
            Some(Ok(event)) = reader.next() => {
                dirty = handle_event(event, js_ctx) || dirty;
            }
            _ = timer_interval.tick() => {
                dirty = poll_timers() || dirty;
            }
            // Optional: async file watcher event
            // Some(event) = hot_reload_rx.recv() => { ... }
        }

        if dirty {
            render_tree(terminal, root_id)?;
            dirty = false;
        }
    }
}
```

## Benefits

- **Zero CPU when idle**: Thread sleeps until an event or timer actually fires
- **Sub-millisecond timer accuracy**: `tokio::time` handles timer wakeups precisely
- **Unified async**: Hot reload file watching, network I/O, and IPC can all use `tokio::select!`
- **Better battery life**: No busy-waiting on laptops

## Considerations

- `rquickjs::Context` is not `Send`, so the event loop must stay on a single thread. This is fine — `tokio::select!` works on the current thread runtime.
- `EventStream` requires `futures::StreamExt`.

## Acceptance Criteria
- [ ] `crossterm::event::EventStream` replaces `poll()`/`read()`
- [ ] `tokio::select!` handles terminal events, timers, and optionally hot reload
- [ ] CPU usage at idle is ~0% (verify with `top`/`htop`)
- [ ] Timer accuracy improved (sub-10ms resolution)
- [ ] All examples work identically
- [ ] `cargo test` passes

## Files to Modify
- `src/event_loop.rs` — Rewrite `run_event_loop` as async
- `src/main.rs` — Call `tokio::runtime::Runtime::block_on()` or use `#[tokio::main]`

## References
- crossterm EventStream: https://docs.rs/crossterm/latest/crossterm/event/struct.EventStream.html
- tokio::select!: https://docs.rs/tokio/latest/tokio/macro.select.html
- Task 013 (Event Loop: tokio::select! — original task, needs revisit)
