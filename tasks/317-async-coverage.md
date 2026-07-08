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

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/built-ins/Promise; tests/test262/test/language/expressions/async-function; tests/test262/test/language/statements/async-function; tests/test262/test/built-ins/AsyncFunction; tests/test262/test/built-ins/GeneratorFunction; tests/test262/test/built-ins/GeneratorPrototype`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All Promise, async-function, and generator areas are active and pass at 100% with zero spec skips.
