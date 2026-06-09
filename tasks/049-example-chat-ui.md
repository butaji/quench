# Task 049: Example: Chat UI

## Goal
Split-pane chat interface using `useStdin`, `useStdout`, `Newline`, and scrolling.

## Acceptance Criteria
- [ ] Source `examples/chat-ui.tsx` with message list (top) and input prompt (bottom).
- [ ] `useStdin` captures typed input; Enter sends message.
- [ ] Messages separated by `Newline`; list scrolls when full.
- [ ] **Reference:** Deno with `npm:ink` produces baseline ANSI output.
- [ ] **TuiBridge:** Same file runs with our rquickjs shim.
- [ ] Parity harness verifies 100% match.
- [ ] Covers: `useStdin`, `useStdout`, `Newline`, split pane, text input handling.

## Dependencies
- Task 032, Task 033, Task 041

## SPEC Reference
§5.3 useStdin / useStdout; §5.2 Components — Newline
