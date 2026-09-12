# 33 — Allocation-site escape analysis for short-lived value objects

Status: planned

Evidence from the V8v7 suite sharpens [[24-escape-analysis-scalar-replacement]] with concrete allocation-site targets rather than a generic rule: raytrace's `Vector`/`Color` prototype methods (`add`, `subtract`, `multiplyScalar`, `multiply`) each allocate a fresh result object that is immediately consumed by the next arithmetic call and never stored or returned past the expression; deltablue allocates `OrderedCollection`/`Plan` objects per propagation step with similarly local lifetimes.

Prioritize the escape-analysis implementation in [[24-escape-analysis-scalar-replacement]] against these two allocation shapes specifically: an object constructed, passed through a short chain of calls, and discarded within the same function activation, with no capture and no store into a longer-lived structure. This gives a concrete, testable first target before generalizing the analysis to arbitrary allocation sites.

Acceptance: raytrace's per-pixel vector/color arithmetic chain compiles with zero heap allocations for intermediate `Vector`/`Color` values, verified by an allocation counter; deltablue's propagation step shows measured allocation reduction for `Plan`/`OrderedCollection` construction; output values (final pixel colors, final constraint values) are bit-identical to the unoptimized path.
