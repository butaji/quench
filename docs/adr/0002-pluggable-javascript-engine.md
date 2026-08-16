# ADR 0002: Pluggable JavaScript engine boundary

## Status

Accepted

## Context

quench-node currently combines Node host behavior with direct `rquickjs`
types and calls. That prevents the same Node API implementation from running
on `quench-runtime`. Engine selection must not change module resolution,
global APIs, error behavior, or Node compatibility.

## Decision

Define an engine-neutral `JsRuntime` contract and keep Node integration in a
separate `NodeHost` layer:

```text
Node declarations and host contract
                ↓
             NodeHost
          ↙           ↘
   QuickJsRuntime   QuenchRuntime
          ↘           ↙
              JsRuntime
```

`JsRuntime` owns JavaScript execution, realms, values, callbacks, jobs,
modules-as-source, buffers, and structured errors. `NodeHost` owns Node
module resolution/loading/cache and OS capabilities. Neither layer exposes
engine-specific values to the other.

The contract uses opaque `JsValue` handles, typed host-function registration,
structured `JsError` values, and shared owned/borrowed `JsBytes`. Runtime
instances are single-thread-affine. Job execution is explicit through
`execute`, `poll_jobs`, `has_pending_jobs`, and shutdown operations. A runtime
is created with its host, and realms are explicitly isolated.

CommonJS and ESM use one host-owned resolver, loader, and module cache. The
existing `rquickjs` implementation is migrated first and remains the behavior
reference. `quench-runtime` is then implemented as a second adapter against
the same contract. API declarations generate both adapters and their tests.

## Consequences

- Direct `rquickjs` types are permitted only inside the QuickJS adapter.
- `quench-runtime` remains independent of both `rquickjs` and `quench-node`.
- Node APIs can be tested once against both engines.
- The initial migration requires an explicit value, callback, promise/job,
  module, buffer, and error boundary instead of a thin `execute()` wrapper.
- Existing Node behavior is preserved by migrating rquickjs first and using
  the existing fixture suite as the compatibility gate.

## Migration order

1. Define the public engine-neutral types and lifecycle contract.
2. Move all current rquickjs lifecycle, loader, conversion, and registration
   code into the QuickJS adapter.
3. Make the current Node host implement the shared host contract.
4. Run the existing Node fixtures unchanged through the adapter.
5. Implement the Quench adapter and compare both engines on the same fixtures.
