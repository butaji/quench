# Goal

Build a high-performance general-purpose JavaScript VM with a Rust-generated
copy-and-patch JIT, optimized and measured on Apple M4/macOS.

Follow the [repository manifesto](AGENTS.md), implement the next ready item in
[the task queue](tasks/index.json), and use the [JIT contract](docs/stencil-jit-implementation-spec.md).
Keep all unfinished host and cleanup work visible without confusing it with the
VM critical path.

V8-v7 is the progress indicator, with Bun/JSC and Node comparisons. Correctness,
startup, compilation time, generated-code size and memory remain explicit gates
or tradeoffs. Neither a score nor passing IR expectations establishes universal
optimality or complete conformance. Follow the [measurement protocol](docs/performance-lanes.md).
