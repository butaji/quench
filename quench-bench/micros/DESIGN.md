# Decisions and glossary

## ADR 1 — Behavioral experiments, not architecture requirements

The corpus compares JavaScript behavior and resource outcomes. Prerequisites
describe evidence needed to interpret a result, not mandatory implementation
steps. A collector, JIT, inline cache, or allocation may legitimately not exist
or disappear through optimization. Diagnostic availability never changes the
qualification verdict. Runtime optimization and adding runtime instrumentation
are outside this delivery's scope.

## ADR 2 — Separate measured outcomes from explanations

Throughput, fixed-work RSS, and instrumented observations come from separate
processes. Independent process pairs are the statistical unit. Exact result
validation and equivalent-variant checks precede causal interpretation. Reports
offer competing explanations and next experiments; they do not manufacture
supported root causes from operation frequency. Existing partial engine traces
remain partial, and unsupported events are explicitly unavailable.

## ADR 3 — Frozen editions and preserved failures

An edition freezes source and protocol hashes. Qualification runs all scenarios
and all gates; an aggregate cannot conceal a loss. Legacy workloads remain
unchanged and explicitly lack reserved variants. Every run is preserved in a
new artifact path. Finite qualification covers declared inputs, not all possible
JavaScript or complete production readiness. This implementation does not add CI
configuration or act on the VM based on findings.

## Terms

- **Experiment:** a behavioral question and its related executable contrasts.
- **Variant:** one deliberately chosen behavior within an experiment.
- **Scenario:** fixed variant, input size, seed, and source form.
- **Control:** reference variant for a descriptive within-engine contrast.
- **Prerequisite:** earlier evidence useful for interpreting a contrast.
- **Sample:** one independently launched process; timing windows are nested.
- **Edition:** immutable identity of a corpus and its measurement protocol.
- **Observed weakness:** measured failure or sensitivity; cause not yet proven.
- **Supported explanation:** a hypothesis corroborated by distinguishing evidence;
  the current runner does not automatically promote timing losses to this level.
- **Validated improvement:** a separately chosen engine change improves outcomes
  and retains semantic/generalization checks; implementation is outside this suite.
- **Unavailable:** the measurement capability did not provide the requested fact.
- **Inconclusive:** measurements exist but do not establish the required outcome.
- **Invalid:** the protocol, data, identity, or execution cannot support a verdict.
- **RSS:** resident process memory, distinct from virtual size and physical footprint.
