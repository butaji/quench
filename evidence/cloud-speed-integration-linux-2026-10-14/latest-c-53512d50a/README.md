# c plain-frame storage integration

Incoming `v2-cloud-c` source `53512d50abac5f6f9c1525092de1c0969e37de31`,
integrated after the retained Richards VFS milestone and native TLS sync.
Preserve the current `Frame::empty`, Wasm parameter window and non-Undefined
register initialization. Import local-slot reachability from the shared
instruction layout and the guarded initialized-register reuse mechanism.

The incoming source predates Wasm lane retirement/materialization. Full
initializers and materialized register copies record the initialized prefix;
continuation transfer clears/derives it; Wasm retirement resets it when shrinking.
All buffer shrink paths bound the remembered prefix. This prevents unsafe
length restoration beyond a retired buffer's capacity. Root-map reuse still
excludes generator/async/non-Undefined activations.

Workspace/all-targets and profile aggregate/memory/trace checks pass. Runtime
583/583 passes with debug assertions and test-only opt-level=0, including the
new Wasm-retirement/register-reuse regression. `integrated-source.patch` is the
exact runtime change relative to the post-TLS source `06c7cb54c`.

Production runners are rebuilt at `9bfe4eabcb029e30cfc58b60b6488cbd8713ec49`;
hashes are in `production-artifacts.json`. Interpreted Wasm passes 67,124/67,124.
Native VFS remains 98/99 and Fs FileHandle 8/19; inventories and every outcome
match the pinned pre-integration control. Test262 expressions/function-code
remain 11,304/11,319 and Function 508/509; all outcomes match the prior checked
artifacts, including the existing failures. No harness or host shims.

The all-eight one-round stock smoke is valid/output-equal for 8/8. It overlaps
semantic suites and its timings are discarded; it is not a performance gate.
No incoming c speed claim is promoted to a Richards win by this integration.
