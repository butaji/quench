> **Minimum custom code.** Use the smallest, most modular parser dependency available.

# Task 337: Evaluate oxc_parser as lower-weight alternative

## Goal

Evaluate whether `oxc_parser` can replace `swc_ecma_parser` to reduce compile times and dependency weight while keeping JS/TS/JSX parsing.

## Rationale

SWC is powerful but pulls in a large platform. Oxc is designed as independent crates (parser, transformer, linter, resolver) and may give us a smaller footprint for a runtime that only needs parsing and type stripping.

## Acceptance criteria

- [ ] Spike branch swaps `swc_ecma_parser` for `oxc_parser`.
- [ ] Existing examples and tests parse successfully.
- [ ] Compile time and dependency count are compared.
- [ ] Decision recorded in `docs/minimum-custom-code-strategy.md`.

## Targets

- **Suite:** `runtime`
- **Batch:** 6
- **Target subset:** n/a (tooling/infrastructure task)
- **Blocked by:** 296
- **Exit criteria:** A documented decision on parser; if Oxc is chosen, the swap lands with no conformance regression.
