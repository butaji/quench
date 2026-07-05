> Sub-task of 296: reach 100% coverage of async functions, Promises, and generators.

# Task 317: Async / Promise / generator coverage

## Status: PENDING

## Goal

100% of test262 `built-ins/Promise/`, `language/expressions/async-function/`, and generator tests pass.

## Scope

- Promise constructor, `then`, `catch`, `finally`
- `Promise.resolve`, `Promise.reject`, `Promise.all`, `Promise.race`
- Microtask queue
- `async`/`await`
- Generators and `yield` (future)

## Acceptance criteria

- [ ] Promise/async tests pass.
- [ ] Fixtures under `tests/spec_fixtures/async/`.

## Dependencies

- Task 251
