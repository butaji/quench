# Quench-node Node.js compatibility gap plan

## Objective

Raise quench-node toward the compatibility level documented by Bun for Node.js v26, while preserving quench-runtime as the only JavaScript runtime and keeping API facts centralized in the existing declaration/registration layers.

Reference: <https://bun.com/docs/runtime/nodejs-compat>

Current evidence: `docs/NODE-COMPAT.md`, `crates/quench-node/src/modules/require.rs`, `crates/quench-node/src/js_runtime_require_module.rs`, `crates/quench-node/src/modules/crypto.rs`, `tests/node-compat/`, and `crates/quench-node-test/node-tests/`.

## Execution order

1. Correct the compatibility inventory before implementation. Add explicit rows for `dns`, `net`, `querystring`, and `https`; split the aggregated Web-global row into independently testable APIs; record the Bun page revision/Node target and regenerate verification counts. Do not change implementation behavior in this step.
2. Implement the TLS/HTTPS baseline. Add the common client/server path, `https.request`, `https.get`, `Agent`, secure socket state, certificate validation behavior, and integration with the existing `net`/`http` host. Match Bun's documented subset first; retain explicit unsupported errors for advanced features not in scope.
3. Implement the common Node crypto baseline. Replace unsupported `createCipheriv`, `createDecipheriv`, and key-generation paths with real behavior for common algorithms required by Node applications and TLS. Keep unsupported algorithm behavior explicit and data-driven. Add focused fixtures for validation, output, errors, and round trips.
4. Replace worker and async-context stubs. Implement `Worker`, `parentPort`, `workerData`, lifecycle, message transfer, and termination using quench-runtime primitives. Define `AsyncLocalStorage` propagation boundaries and add tests for worker/message-port behavior before adding advanced hooks.
5. Expand `node:module` and loader compatibility. Add the supported ESM/module APIs, resolution and registration hooks, package lookup, and source-map behavior that can be implemented without a second syntax tree or runtime. Preserve the existing CJS resolver and migrate callers through the shared declaration layer.
6. Implement diagnostics and performance surfaces. Prioritize `diagnostics_channel`, then complete meaningful `perf_hooks`, `trace_events`, `inspector`, and `v8` subsets. Separate real behavior from shape-only stubs and document unavoidable engine-specific differences.
7. Complete Web-platform globals and stream interop. Cover Web Streams, BYOB readers/controllers, encoder/decoder streams, message-port transfer behavior, performance resource entries, Request option semantics, and remaining SubtleCrypto algorithms where supported by the existing runtime.
8. Expand Node developer tooling. Improve `node:test` in-process behavior, then REPL, WASI, reporters, mocks, snapshots, and CLI integration according to observable compatibility contracts.

## Per-task acceptance contract

- Model reusable API facts in the existing shared declaration/IR layer first.
- Generate mechanical registrations/wrappers; handwrite only observable algorithms.
- Add a focused stage under `tests/node-compat/stage-N/` for each new observable contract.
- Run the focused fixture and the relevant existing compatibility command.
- Format with Prettier and run `git diff --check` after the change.
- Update `docs/NODE-COMPAT.md` with measured status and remaining differences.
- Do not add alternate JavaScript runtimes, CI configuration, or unrelated external-project changes.

## Prioritization rationale

- P0 networking, crypto, and workers block major Node application classes and framework behavior.
- P1 loader, diagnostics, globals, and tooling improve package ecosystem compatibility after core execution paths are sound.
- `node:sea` is not a priority gap because Bun also documents it as unsupported and recommends a Bun-specific executable path.

## Risks

- TLS and crypto implementations can silently diverge in validation, error codes, stream ordering, and key material. Every supported path needs Node CLI behavior as the oracle before coding.
- Worker and async-context support can expose lifecycle and heap-reference bugs. Keep references compact and make ownership/disposal explicit.
- Engine-specific V8 behavior must not be faked. Return guarded/unknown behavior or explicit unsupported errors where semantics cannot be provided correctly.
- The current matrix contains omissions and historical test counts; inventory correction must precede interpreting percentage progress.
