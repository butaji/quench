# Task 013: Event Loop tokio::select!

## Goal
Build the blocking event loop with tokio::select! over crossterm, timers, and reload channel.

## Acceptance Criteria
- [ ] `tokio::main` sets up `EventStream`, `InkVm`, terminal.
- [ ] `select!` branches: crossterm event, timer_rx, reload_rx.
- [ ] After each event batch, single `terminal.draw()` if dirty.
- [ ] Frame rate cap not needed (event-driven); loop blocks on `select!`.
- [ ] Unit test: inject synthetic events via channels, verify draw called once per batch.

## Dependencies
- Task 001

## SPEC Reference
§3.3 Event Loop: Event-Driven, Zero Polling
