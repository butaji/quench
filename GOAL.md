# Goal

Reach 100% ECMA-262 test262 conformance for the pinned submodule revision in
`tasks/index.json`, with no undocumented skips and minimal handwritten Rust.
The only architectural plan is [docs/architecture.md](docs/architecture.md).

Quench reduces OXC syntax and semantic data using unified `Proven`, `Guarded`,
and `Unknown` facts. It executes only residual uncertainty using one semantic
operation algebra, macro-generated physical specialization, and compact heap
references. Preserve observable ECMAScript behavior before optimizing.

The stage run is the progress SSOT. Commit and push each finished verified
slice.
