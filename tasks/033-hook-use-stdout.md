# Task 033: Hook useStdout

## Goal
Implement `useStdout` providing write access and terminal dimensions.

## Acceptance Criteria
- [ ] `useStdout()` returns `{write, columns, rows}`.
- [ ] `write(data)` delegates to `__ink_stdout_write`.
- [ ] `columns` / `rows` reflect current terminal size.
- [ ] Updates on terminal resize.
- [ ] Unit test: write to stdout, verify bytes in buffer.

## Dependencies
- Task 031

## SPEC Reference
§5.3 useStdout
