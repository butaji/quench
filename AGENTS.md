# Repository rules

Quench implements JavaScript semantics and Node-compatible APIs.

- Use Rust for implementation and Rust macros for code generation;
  do not introduce other implementation or code-generation languages.
- Keep Node host/API behavior in `quench-node` and JavaScript semantics
  in `quench-runtime`.
- OXC owns syntax; do not introduce a competing syntax tree.
- Represent semantic facts once. Derive repetitive registration and
  wrappers from explicit declarations when this reduces complexity;
  keep exceptional behavior visible.
- Resolve static structure before runtime where practical. Justify
  added allocations, metadata, caches, and generated or native code
  by their measured benefit or semantic necessity.
- Share semantics across interpreter, specialized, and native execution.
  Specialization must preserve all observable JavaScript behavior.
- Establish correct general behavior before fast paths. Optimizations
  require proven assumptions or runtime guards with a correct fallback,
  and must apply beyond the workload that motivated them.
- Verify changed Node behavior against the local Node oracle and
  relevant upstream source. Compare values, descriptors, identity,
  ordering, errors, exit status, and host effects as applicable.
- Benchmarks measure production behavior. Never select production
  behavior using benchmark identity, fixture source, scores, checksums,
  suite markers, or the comparison engine.
