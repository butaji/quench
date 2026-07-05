# Alignment

## Goal

Reach **100% compatibility with JavaScript, TypeScript, TSX, and JSX** executing **natively** (no transpilation, no bytecode, no JIT), with the **minimum amount of code** and the **maximum possible performance**.

## How we get there

- **Low effort / high impact first.** Every task is ordered by effort vs. payoff.
- **No stubs.** Unimplemented behavior throws a clear error.
- **Spec-driven.** Every feature matches ECMA-262 / TypeScript / JSX semantics.
- **Complete fast test coverage.** Every spec behavior has a focused Rust unit test or fixture.

## Documents

- `EXECUTE.md` — principles and decision filter.
- `docs/js-ts-compatibility-roadmap.md` — P0/P1/P2 roadmap to 100% compatibility.
- `docs/runtime-optimization-roadmap.md` — performance and code-size review.
- `docs/spec-test-fixtures.md` — fixture convention for spec coverage.
- `docs/conformance.md` — current conformance numbers and full-suite target.

## Task order

See `tasks/index.json`. Top of the queue:

1. 296 — 100% compatibility tracker
2. 298 — complete fast Rust unit test coverage
3. 297 — spec test fixtures
4. Measurement (253, 91, 250, 97)
5. Quick wins (291, 281, 283, 286, 284, 282, 290, 289, 292, 293, ...)
6. Big architecture (85, 241, 182, 251, 88, 264, 82)
