# ADR 0002: Pluggable JavaScript engine boundary

## Status

Accepted

## Context

quench-node now uses `quench-runtime` as its sole JavaScript engine. The
engine-neutral contract remains valuable for keeping Node host behavior
independent from engine-specific values and calls.

## Decision

Define an engine-neutral `JsRuntime` contract and keep Node integration in a
separate `NodeHost` layer:

Node declarations and host contract
                ↓
             NodeHost
                ↓
           QuenchRuntime
                ↓
             JsRuntime

`JsRuntime` owns JavaScript execution, realms, values, callbacks, jobs,
modules-as-source, buffers, and structured errors. `NodeHost` owns Node
module resolution/loading/cache and OS capabilities. Neither layer exposes
engine-specific values to the other.

The contract uses opaque `JsValue` handles, typed host-function registration,
structured `JsError` values, and shared owned/borrowed `JsBytes`. Runtime
instances are single-thread-affine. Job execution is explicit through
`execute`, `poll_jobs`, `has_pending_jobs`, and shutdown operations. A runtime
is created with its host, and realms are explicitly isolated.

CommonJS and ESM use one host-owned resolver, loader, and module cache.
`quench-runtime` implements the contract, and API declarations generate the
shared adapter and its tests.

## Consequences

- `quench-runtime` remains independent of `quench-node` and owns JavaScript
  execution semantics.
- Node APIs are tested through the sole runtime.
- The migration requires explicit value, callback, promise/job, module, buffer,
  and error boundaries instead of a thin `execute()` wrapper.
- Existing Node behavior remains the compatibility gate.

## Migration order

1. Define the public engine-neutral types and lifecycle contract.
2. Keep Node host behavior behind the shared contract.
3. Run the existing Node fixtures unchanged through quench-runtime.
4. Compare compatibility results against the upstream Node behavior.
