# ADR 0001: Node 24 broad compatibility runtime contract

## Status

Accepted

## Decision

quench-node is a broad Node-compatible runtime built on Rust and rquickjs. Its
target is Node 24 behavior on Linux x86_64, Linux ARM64, macOS, and Windows.
Native addons and Node-API are outside the contract.

Applicable Node upstream tests are the primary oracle. Native-addon and
explicitly platform-impossible tests are the only exclusions. Required
application gates cover web server, CLI, package loader, streams, crypto, and
database/SDK workloads. Compatibility statuses are defined in
[`compatibility-contract.md`](../compatibility-contract.md).

## Release criterion

Every supported platform must have no unexplained failures in the declared
Node 24 contract, all application gates must pass, and two complete runs must
finish with zero retries or timeouts. A reproducible report must include the
runtime version, Node oracle, platform, manifest revision, fixtures,
applications, and results.

## Boundaries

Declarations and generated JavaScript adapters own ordinary Node semantics;
handwritten JavaScript is reserved for irreducible behavior. Rust owns the
declaration/IR generator, engine integration, and unsafe or OS-bound
primitives. No separate `quench-runtime` crate may be introduced. TypeScript
execution and package installation are outside the runtime contract;
applications start after dependencies are installed.

LLRT remains a QuickJS/Rust design reference, WinterJS a web-platform and
capability-matrix reference, Deno a foreign-runtime runner reference, WPT a
web-platform oracle, and Test262 an ECMAScript baseline.

## Consequences

- API names alone do not establish compatibility; every entry needs evidence.
- Unsupported and platform-limited behavior remains visible and deterministic.
- Real applications block releases alongside upstream tests.
- All supported platforms share one core contract.
