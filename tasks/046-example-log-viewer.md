# Task 046: Example: Log Viewer

## Goal
Real-time log viewer using `Static` component with auto-scrolling.

## Acceptance Criteria
- [ ] Source `examples/log-viewer.tsx` appends log lines every second via `useEffect` + `setInterval`.
- [ ] Uses `Static` for log lines; main tree shows status bar.
- [ ] Deno + TuiBridge parity harness verifies 100% match.
- [ ] Covers: `Static`, `useEffect`, timers, scrolling-like behavior, split layout.

## Dependencies
- Task 027, Task 017, Task 041

## SPEC Reference
§3.2 Renderer — Static; §5.2 Components
