# Shared OXC RegExp parser stack integration

Pinned OXC `oxc_regular_expression` 0.47.1, archive SHA256
`d495804e1bf588e5e1a2d6fe760ff1ba53ece5cbf04e4dce531da64014869fea`, upstream revision
`8f5be07ed60cf4228dc94709fa0ae3f62096b704`, path `crates/oxc_regular_expression`.
The matching upstream MIT license is retained. OXC owns RegExp grammar.

Both runtimes consume this parser through quench-regexp. Recursive group and
UnicodeSets class parsing consume the shared quench-stack transition budget.
The archive lockfile pins standalone upstream test dependencies; production uses
the repository workspace lockfile.

The resource diagnostic has a named error code and a public kind predicate,
independent of its text. QUENCH.patch records source changes against the archive
as a zero-context unified diff (`git apply --unidiff-zero`). Run upstream tests
with `cargo test --manifest-path vendor/oxc-regexp/Cargo.toml --lib`.

This package is not yet selected by the production workspace. Runtime adapters,
AST lowering and matching recursion require separate integration and validation.
Node's UnicodeSets parser exhaustion is a SyntaxError; runtime adapters must
preserve that distinction from ordinary guest call-stack RangeError.
