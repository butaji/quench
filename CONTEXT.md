# Runtime map

- `quench-runtime` owns language execution and JavaScript semantics.
  JS reduction/compact execution uses `reduce`, `ir`, `machine` and `vm`;
  shared typed execution also has `hir`, `mir` and `interp`.
- `quench-node` owns Node APIs and host effects.
- `quench-wasm` decodes/validates Wasm and adapts spec scripts to the runtime.
- Native, Fast and Dynamic describe execution/representation facts, not separate
  languages or permission to introduce alternate semantics.
- Do not describe all current storage as one tracing collector: the runtime
  contains both GC machinery and JS reference-cycle collection. Respect the
  actual ownership/rooting contract of each value.
- Desired architecture and verified implementation are distinct.
  [Repository rules](AGENTS.md) apply throughout.

Active work: [task queue](tasks/index.json).
Implementation contract: [stencil specification](docs/stencil-jit-implementation-spec.md).
Manuals and verification: [documentation index](docs/README.md).
