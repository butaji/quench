> Task for establishing the spec-fixture testing convention. No runtime code changes.

# Task 297: Establish spec test fixtures

## Status: IN PROGRESS

## Goal

Ensure every JS/TS spec-compatibility gap is tracked with a focused fixture test under `crates/quench-runtime/tests/spec_fixtures/`, so coverage is fast, granular, and maps directly to spec sections.

## Outputs

- `docs/spec-test-fixtures.md` — fixture format, directory layout, and running instructions.
- `crates/quench-runtime/tests/spec_fixtures/` — directory tree with example fixtures for top compatibility gaps.
- `tasks/index.json` — every compatibility task notes the fixture files it expects to add/fix.

## Fixture layout

```text
crates/quench-runtime/tests/spec_fixtures/
├── expressions/
├── statements/
├── functions/
├── objects/
├── arrays/
├── classes/
├── modules/
├── typescript/
└── errors/
```

## Fixture format

```js
// spec: ecma-262 sec-typeof-operator-runtime-semantics
// expect: value: "undefined"
// tags: typeof, undeclared

typeof notDeclared;
```

## Convention rules

1. One behavior per fixture.
2. Expected result is encoded in the metadata comment.
3. Each fixture maps to an ECMA-262 or TypeScript spec section.
4. A fixture is either passing, skipped with a task id, or failing until the runtime fix lands.
5. Fixtures are run by a future Rust harness; this task only defines the convention and data.

## Verification

- [ ] `docs/spec-test-fixtures.md` is complete.
- [ ] Directory tree exists with representative fixtures.
- [ ] Open compatibility tasks in `tasks/index.json` reference fixture paths.

## Dependencies

- Task 296 (100% compatibility tracker)
- All P0/P1 compatibility tasks
