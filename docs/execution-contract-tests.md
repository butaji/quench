# Execution-profile contracts

The corpus is `crates/quench-runtime/testdata/execution_profiles/`.
It contains 342 JavaScript/JSON pairs. The Rust harness owns parsing,
execution, verification and expectation comparison.

```json
{
  "schema": 3,
  "contract": "optimized",
  "warmup": 1,
  "result": { "kind": "number", "value": 7 },
  "ir": ["LoadLocal", "LoadLocal", "Add", "Return"]
}
```

`contract: "optimized"` names the execution-profile suite. The `ir` array is the
single canonical semantic hot-IR target produced by the current lowering policy,
and Quench must reproduce it. Quickened aliases belong only to the raw physical
witness. This is the best *verified target for that policy*, not a proof that a
different lowering or dataflow cannot be better. Passing this target does not by itself prove globally
optimal IR or machine code; those claims require a changed lowering contract,
independent semantic evidence, native-entry witnesses and production measurements.
This example
illustrates the schema, not a new fixture. Read actual warmup and
verification behavior in `test_execution_profile_tests.rs::execute_case`:
warmup executes the contract program; a fresh initialization prepares `run`,
its arguments and verification; the harness captures reachable IR and invokes
the verifier. Do not equate this automatically with warming one persistent
function instance.

The current `hot_ir` traverses reachable PCs and `canonical_hot_opcode` maps
generic `Binary` flags and selected `Slow` payloads to canonical opcode names.
The JSON array therefore does not prove dedicated physical opcode emission,
dynamic hotness, nested-body coverage, complete operand/dataflow optimality,
or machine-code quality. Stencil entries and fallback counters are deliberately
outside this IR-only corpus; they are observed only by targeted execution tests
and do not create a second JSON expectation set. They remain implementation
work on the copy-and-patch JIT critical path.

The raw witness records each instruction's PC, opcode, flags, operands and
branch target, plus sorted immutable code-store IDs for the run body and nested
structured bodies. Warmups intentionally execute isolated contract instances;
the harness does not claim that a stateful function instance is reused across
warmup and measurement until that policy has its own proof.

These boundaries are normative for the task queue: “342 green” means the canonical
hot-IR target and independent result contracts pass. This document is also the
single current-status authority for architecture-specific execution modes. The
current inventory has no generic `Binary` or
generic `Slow` rows for the observed corpus, but raw physical instructions
remain diagnostic for native qualification. The generated lowering map
(`Op::LOWERING_MATRIX` and `Op::physical_opcode`) and its
production dispatch consumers are part of the existing foundation; native entry
and machine-code quality remain later JIT evidence. See [task evidence boundaries](../tasks/README.md#evidence-boundaries).

Run the aggregate contract check:

```sh
cargo test -p quench-runtime test_execution_profile::tests::every_json_contract_matches_hot_ir -- --nocapture
```

This command uses the default execution policy. Architecture-specific stencil
opt-ins are separate diagnostic configurations: they may change quickening or
physical execution and must not be treated as another JSON expectation set.
Such a mode must first pass this same 342-case contract check before it can be
considered for production qualification.
Quickening aliases are normalized through the generated `semantic_opcode` view,
so the AArch64 `leaves` mode uses the same canonical IR contract as the default
policy. The status matrix is deliberately small and data-first:

| Execution policy | 342 semantic contracts | Production qualification |
| --- | --- | --- |
| default host policy | green | Apple-arm remains diagnostic until native-entry and M4 evidence qualify it; other supported hosts follow their own native-entry evidence |
| AArch64 `leaves` | green | diagnostic; native-entry and M4 qualification open |
| AArch64 `fusion-numeric` | green | diagnostic; native-entry and M4 qualification open |
| AArch64 `all` | green | diagnostic; native-entry and M4 qualification open |

These policies share one JSON expectation set. A policy may not create an
alternate expectation or turn semantic green into a speed claim; qualification
requires the native-entry witnesses and M4 measurements described in
[the performance protocol](performance-lanes.md).

To produce the current physical-lowering inventory without changing the JSON
contract, use the diagnostic-only switch:

```sh
QUENCH_EXECUTION_PROFILE_PHYSICAL_INVENTORY=1 \
  cargo test -p quench-runtime test_execution_profile::tests::every_json_contract_matches_hot_ir -- --nocapture
```

It groups reachable top-level instructions by their raw opcode and, for generic
`Binary`, by its flags. Typed cold markers are reported under their dedicated
opcodes, including structured and host variants whose handlers remain canonical
named fallbacks. The generic `Slow` row is reserved for operations without a
fixed-width contract, unclassified variants and legacy decoding. The output is
a physical-lowering inventory, not a desired physical expectation and not a
performance score. The harness separately captures
nested code-store identities for the raw witness; those identities are not
folded into this top-level instruction count.

The physical-witness milestone separates actual physical emission from semantic
normalization. The active JIT work consumes that inventory to close generated
lowering and account for unobserved variants. Keep all 342 cases and their intended
semantics; do not regenerate desired expectations from current output just to
make them pass. The canonical hot-IR JSON is one expectation authority; raw
physical output remains a diagnostic witness for native qualification. Generic `Slow` witnesses carry the canonical
`Op` variant name as their fallback boundary; typed cold rows must never be
reported as anonymous generic fallbacks.

For every optimization, test semantic boundaries and adversarial input variants:
signed zero, NaN, overflow/conversion, changed types, aliases, descriptor/prototype
mutation, coercion effects, throws and exact resumption as applicable.
Use independent result expectations and the local Node oracle.

There is no implemented general execution-contract DSL promised by this document.
New counters must derive names, units and populations from shared Rust declarations.
Missing observations never prove zero work. Native and ownership tests supplement
these fixtures; [production measurements](performance-lanes.md) establish speed.
