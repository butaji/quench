> Convention for spec-based unit-test fixtures.

# Spec Test Fixtures

To reach 100% JS/TS/TSX/JSX compatibility we exercise the runtime against real language snippets. Each snippet is a **fixture**: a small `.js` or `.ts` file that tests one observable spec behavior.

## Goals

- **Complete coverage.** Every JS/TS spec behavior that the runtime claims to support has a fixture.
- **One behavior, one fixture.** A fixture should fail for exactly one reason.
- **Fast.** Fixtures run as Rust unit tests via `cargo test -p quench-runtime spec_fixtures` and complete in seconds.
- **Self-describing.** The expected result is encoded in the fixture file itself.
- **Aligned with specs.** Every fixture maps to an ECMA-262 or TypeScript language section.

## Layout

```text
crates/quench-runtime/tests/spec_fixtures/
├── expressions/
│   ├── typeof_undeclared.js
│   ├── nullish_coalescing.js
│   ├── optional_chaining.js
│   └── delete_operator.js
├── statements/
│   ├── while_loop.js
│   ├── for_of_loop.js
│   └── try_catch_throw_object.js
├── functions/
│   ├── arguments_object.js
│   ├── default_parameters.js
│   └── rest_parameters.js
├── objects/
│   ├── property_descriptor.js
│   ├── prototype_chain.js
│   └── constructor_registration.js
├── arrays/
│   ├── array_length.js
│   ├── array_spread.js
│   └── array_builtin_no_clone.js
├── classes/
│   ├── class_constructor.js
│   └── static_fields.js
├── modules/
│   └── import_export.js
├── typescript/
│   ├── enum_numeric.js
│   ├── type_assertion_erasure.js
│   └── as_const.js
└── errors/
    ├── error_instanceof.js
    └── thrown_value_preserved.js
```

## Fixture format

A fixture is a `.js` or `.ts` file with a metadata comment at the top:

```js
// spec: ecma-262 sec-runtime-semantics-typeof
// expect: value: "undefined"
// tags: typeof, undeclared

typeof notDeclared;
```

Supported directives:

- `spec:` — ECMA-262 or TypeScript spec section.
- `expect:` — `value: <json>` or `error: <type>` or `console: <string>`.
- `tags:` — comma-separated keywords for filtering.
- `skip:` — reason the fixture is currently skipped.

## Running fixtures

```bash
# all fixtures
cargo test -p quench-runtime --test spec_fixture_runner

# by tag
cargo test -p quench-runtime --test spec_fixture_runner -- --tag typeof

# fail fast
cargo test -p quench-runtime --test spec_fixture_runner -- --fail-fast
```

## Adding a new fixture

1. Create the file under the correct category.
2. Add the metadata comment.
3. Run the runner; if it fails, either fix the runtime or mark `skip:` with a task id.
4. Do not commit fixtures that silently pass with wrong behavior.

## Link to tasks

Every open compatibility task in `tasks/index.json` should reference the fixture files it expects to add or fix.
