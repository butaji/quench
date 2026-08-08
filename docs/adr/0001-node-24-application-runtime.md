# ADR 0001: Node 24 application runtime contract

## Status

Accepted

## Decision

quench-node is an application-first Node-compatible runtime built on Rust and
rquickjs. Its compatibility target is Node 24 behavior on Linux x86_64.

Compatibility means matching observable APIs, module loading, errors, and
timing wherever the host platform permits it. Platform-bound behavior is not
silently treated as compatible; it is classified explicitly.

Node's upstream tests are the primary compatibility oracle. The initial
manifest covers `test/parallel/`, `test/es-module/`, and the required
`test/common/` and `test/fixtures/` support files. A curated application gate
is also required for release confidence, beginning with a Hono application and
a small npm CLI tool. Each application is compared under Node 24 and
quench-node, including status, headers, and body where applicable.

Compatibility status is stored in a versioned JSONC manifest under
`tests/node-compat/`. It supports `pass`, `fail`, `skip`, `platform-limited`,
`unsupported`, and `known-conflict`, with explicit fixture entries and optional
prefix defaults.

## Release criterion

A Linux x86_64 release must have zero failures in the curated application gate
and no regressions in the declared Node 24 compatibility manifest.

## Boundaries

The authoritative test sources and their roles are documented in
[`authoritative-test-sources.md`](../authoritative-test-sources.md).

Rust remains limited to engine integration and unsafe or OS-bound primitives.
Node semantics remain in readable JavaScript polyfills. LLRT is a design
reference for QuickJS/Rust boundaries and capability classification. Deno's
Node compatibility runner is a design reference for manifests and foreign
runtime test execution. WPT covers web-platform APIs; Test262 remains a
secondary engine-baseline check.

## Consequences

- Upstream test counts are evidence of progress, not an API-coverage
  percentage.
- Platform limitations and upstream fixture conflicts remain visible and
  auditable.
- Real npm applications can block a release even when focused fixtures pass.
- The existing `tests/node` submodule and local runners should be extended,
  rather than replaced with a second vendored suite.
