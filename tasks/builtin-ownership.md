# Builtin ownership guardrail

Each builtin operation has one owner. Observable ECMAScript algorithms,
including public methods, constructor behavior, prototype behavior, coercion,
validation, ordering, iteration, and descriptors, belong in `builtins/*.js`
whenever they can use `__ops__`. Rust is reserved for interpreter/core
operations, canonical `__ops__` primitives, storage and native memory,
performance-sensitive or crate-backed primitives, engine integration, and
explicit lower-LOC direct bindings listed in
`tasks/builtin-direct-bindings.txt`.

Ownership moves and guardrail-driven cleanups are refactors. Defer them until
the complete configured Test262 corpus is at 100% (zero failures and zero
skips), unless a minimal ownership correction directly fixes an observed
conformance failure. Any allowed broad ownership change must begin and end
with a complete Test262 run at 100%.

Rust-owned public builtin methods must carry an adjacent `@builtin-rust Name`
marker. A JS prototype assignment with the same name then fails the ownership
check unless it is listed as an intentional one-line proxy. This marker is
deliberately explicit: the checker must fail closed instead of inferring
ownership from arbitrary Rust identifiers.

Run the guardrail with:

```sh
bash tools/check-builtin-ownership.sh
```

When moving an implementation, delete the Rust marker and direct-binding entry
in the same change that adds the JS algorithm. Do not add an exception for a
real JS algorithm; exceptions are only for wrappers whose JS body is one line.
