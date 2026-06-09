# Task 047: Example: Spinner

## Goal
Animated spinner with `useEffect`, `setInterval`, conditional rendering, color cycling.

## Acceptance Criteria
- [ ] Source `examples/spinner.tsx` cycles through spinner frames every 100 ms.
- [ ] Color changes each frame (`color` prop dynamic).
- [ ] **Reference:** Deno with `npm:ink` produces baseline ANSI output.
- [ ] **TuiBridge:** Same file runs with our rquickjs shim.
- [ ] Parity harness verifies 100% match.
- [ ] Covers: rapid timer-driven re-renders, dynamic styles, conditional text.

## Dependencies
- Task 041

## SPEC Reference
§3.3 Event Loop — timers; §3.2 Renderer
