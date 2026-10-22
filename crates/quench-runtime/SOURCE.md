# Source provenance

The runtime source is based on `../v2/crates/vm` at reference commit
`e511d1f7e401ecff8cfc4f7cccb6292e6426be44`.

The copy is intentionally self-contained: production builds must not resolve a
sibling checkout. The Rust module layout, bytecode format, OXC specializer,
heap, host ABI, VM, profiling modules, and reference tests are preserved. This
provenance does not create a build dependency on the sibling checkout.
