# Task 037: DevEx: File Watcher

## Goal
Integrate `notify` + `esbuild --watch` to detect plugin changes.

## Acceptance Criteria
- [ ] `notify` watches `plugins/**/*.tsx` (or configured glob).
- [ ] `esbuild --watch` rebuilds bundle on change (< 20 ms).
- [ ] Rust receives path via `reload_rx` channel.
- [ ] Integration test: touch a `.tsx` file, verify rebuild event received in < 50 ms.

## Dependencies
- Task 001

## SPEC Reference
§6 Hot Reload
