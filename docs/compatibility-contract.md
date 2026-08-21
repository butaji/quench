# Quench Node compatibility contract

## Target

The release target is broad Node 24 compatibility across Linux x86_64, Linux
ARM64, macOS, and Windows. Native addons and Node-API are explicitly outside
the contract. TypeScript execution and package installation are outside the
runtime contract; applications begin after dependencies are installed.

Broad compatibility includes applicable Node 24 modules, globals, CommonJS,
ES modules, package resolution, event-loop behavior, filesystem, networking,
process control, concurrency, diagnostics, VM APIs, `node:test`, and Node web
platform APIs. This includes `worker_threads`, `cluster`, `child_process`,
`vm`, `v8`, and `inspector` unless explicitly classified below.

## Compatibility statuses

- `pass` — behavior matches the Node 24 oracle.
- `fail` — an observed mismatch blocks completion.
- `unsupported` — unavailable, with a deterministic Node-shaped error.
- `platform-limited` — unavailable on a supported platform or host setup.
- `known-conflict` — an upstream fixture conflicts with the declared target or
  has a verified harness issue.
- `not-tested` — no evidence exists; never counted as passing.

## Release gates

1. All applicable Node 24 tests pass or are explicitly classified. Native-addon
   and explicitly platform-impossible tests are the only exclusions.
2. Application gates cover web server, CLI, package loader, streams, crypto,
   and database/SDK workloads.
3. The same core contract passes on every supported platform.
4. Two complete verification runs have zero unexplained failures, retries, or
   timeouts.
5. A reproducible report records runtime version, Node oracle, platform,
   manifest revision, fixtures, applications, and results.

Performance has no absolute parity target; severe regressions are prohibited
and measurements are published separately. Permissions, filesystem/network
access, environment access, and resource limits are part of the contract.

## Implementation boundary

API declarations and generated JavaScript adapters own ordinary Node semantics.
Handwritten JavaScript owns only irreducible behavior. Rust remains limited to
the declaration/IR generator, QuickJS integration, and unsafe or OS-bound
primitives. Do not introduce or depend on a separate `quench-runtime` crate.
The minimum-maintainable-LOC architecture is documented in
`data-first-minimal-runtime.md`.
