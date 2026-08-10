# Data-first minimal-LOC migration

> This architecture governs every task in this directory. A task may retain
> historical evidence, but its next implementation must follow this rule.

## Goal

Minimize total maintainable LOC while preserving observable Node behavior.

## Rule

Model API facts as declarations and normalize them into a shared IR. Generate
repetitive module registration, exports, argument validation, error mapping,
sync/async and callback/promise adapters, and routine tests. Keep handwritten
JavaScript only for irreducible compatibility behavior. Keep Rust limited to
the generator, rquickjs integration, and unsafe or OS-bound primitives.

Prefer one generic adapter and one compact declaration over duplicated wrappers.
Do not minify or obscure the declarations or exceptional behavior. Generated
artifacts are outputs and must not become a second source of truth.

## Required decision for every task

Before adding code, record whether the change belongs in:

1. shared declaration/IR data;
2. a reusable generated adapter;
3. a Rust host primitive; or
4. irreducible handwritten JavaScript.

If it is category 4, explain why categories 1–3 cannot express it. Existing
task history is evidence, not permission to extend a duplicated pattern.

See [`docs/data-first-minimal-runtime.md`](../docs/data-first-minimal-runtime.md).

## Status

Adopted as the governing architecture for all compatibility tasks.
