# Source provenance

This crate is a source copy of `../v2/crates/vm` at reference commit
`e511d1f7e401ecff8cfc4f7cccb6292e6426be44`.

The copy is intentionally self-contained: production builds must not resolve a
sibling checkout. The Rust module layout, bytecode format, OXC specializer,
heap, host ABI, VM, profiling modules, and reference tests are preserved. The
package name is changed only so this snapshot can coexist with the legacy
`quench-runtime` crate during the staged rewrite.
