# Goal: Node API compatibility in pure Rust

Implement Node API compat in pure Rust.

- Lisp-mindset and total DRY.
- Acceptance is defined by [`docs/STAGES.md`](docs/STAGES.md),
  [`docs/NODE-STAGES.md`](docs/NODE-STAGES.md), and the examples under
  `crates/quench-node-test/examples/`.
- `quench-node-test` runs Node API tests from the submodule (with a parallel folder from the Node test suite).
- `quench-runtime` is a pure JS engine.
- Define a few web app examples in `quench-node-test/examples` to run real npm-based apps (Hono, Express, Next.js) to make sure the implementation is complete.
- Very clear boundaries!
