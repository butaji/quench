# Stage 01 — Foundations

## Goal
Close the pure, allocation-light APIs that every later fixture imports: `assert`, `buffer`, `console`, `util`, `string_decoder`, and `punycode`.

## Implementation
- Inventory declarations and exports in `crates/quench-node/src`; extend the shared declaration/IR layer before Rust registration or wrappers.
- Reconcile constructor/function duality, constants, prototype ownership, coercion, error names/codes, and cross-realm behavior against local Node.
- Generate mechanical registration and ordinary wrappers; handwrite only encoding, formatting, and assertion algorithms.

## Oracle and acceptance
- Run matching `tests/node/test/parallel/test-{assert,buffer,console,util,string-decoder,punycode}*.js` where present.
- Add focused fixtures under `tests/node-compat/stage-N/` only for uncovered observable contracts.
- Verify `cargo run -p quench-node-test --bin run-compat -- --quiet`; no skips for this scope.

## Exit criteria
All exported names resolve through both `node:` and legacy forms, invalid arguments produce Node-compatible errors, and downstream stages can use Buffer/string decoding without bespoke shims.
