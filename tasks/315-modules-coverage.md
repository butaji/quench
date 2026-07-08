> Sub-task of 296: reach 100% coverage of ECMA-262 modules.

# Task 315: Modules coverage

## Status: PENDING

## Goal

100% of test262 `language/module-code/` and TypeScript module tests pass.

## Scope

- `import`/`export` declarations
- Default, named, and namespace imports/exports
- Cyclic module graphs
- Module-level scope isolation
- TypeScript `moduleResolution` baselines

## Acceptance criteria

- [ ] Module tests pass.
- [ ] Fixtures under `tests/spec_fixtures/modules/`.

## Dependencies

- Task 241

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/language/module-code; tests/test262/test/language/import; tests/test262/test/language/export`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All test262 module-code, import, and export areas are active and pass at 100% with zero spec skips.
