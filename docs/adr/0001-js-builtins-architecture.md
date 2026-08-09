# ADR 0001: Superseded — self-hosted JS builtins

- Status: superseded by ADR 0005.

This historical proposal made JS a default implementation language for
builtins and used a `__ops__` bootstrap bridge. It is not an implementation
plan. Builtins are now declared runtime data with generated primordial
installation; their algorithms are readable Rust handlers.
