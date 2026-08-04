# Builtin ownership guardrail

Each builtin algorithm has one owner. Spec algorithms belong in `builtins/*.js`;
Rust is reserved for the core, performance-sensitive primitives, and explicit
one-line proxy bindings listed in `tasks/builtin-direct-bindings.txt`.

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
