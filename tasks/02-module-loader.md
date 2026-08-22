# Stage 02 — Module loader

Implement one declaration source for builtin names, `node:` aliases, CJS/ESM resolution, package `exports`/`imports`, conditions, `createRequire`, `require.resolve`, cache identity, and CJS↔ESM boundaries. Reconcile `registry.rs`, `js_runtime_capabilities.rs`, polyfill abilities, and `modules/require.rs` so generated registration cannot drift.

Run upstream `test/parallel` module fixtures and `test/es-module`; add focused resolver fixtures for every discovered mismatch. Acceptance: builtin/subpath identity, package maps, errors, cycles, and cache behavior match local Node; `run-compat --quiet` passes for the cluster with no silent fallback.
