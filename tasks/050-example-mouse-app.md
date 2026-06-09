# Task 050: Example: Mouse App

## Goal
Full-screen app demonstrating mouse clicks, hit testing, and mouse-aware `useInput`.

## Acceptance Criteria
- [ ] Source `examples/mouse-app.tsx` with clickable buttons and hover highlighting.
- [ ] Mouse events received via `useInput` (Ink does not have `useMouse`; mouse comes through input handler when enabled).
- [ ] Clicking button toggles its state; hover changes `backgroundColor`.
- [ ] **Reference:** Deno with `npm:ink` produces baseline ANSI output.
- [ ] **TuiBridge:** Same file runs with our rquickjs shim.
- [ ] Parity harness verifies 100% match.
- [ ] Covers: mouse events, hit testing, dynamic style updates.

## Dependencies
- Task 015, Task 041

## SPEC Reference
§7.3 Hit Testing (Mouse); §4 Bridge API — register_input
