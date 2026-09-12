# 91 — Map/Set open-addressing hash-table stencils

Status: planned

`Map`/`Set` are not yet in the host surface listed in the README, and a naive
implementation of them (a boxed hash map behind a generic host call) pays a full
host-call boundary crossing plus generic hashing/boxing for every `get`/`set`/`has`/
`add`, when the common key types (small integers, interned strings — see
[[11-string-representation]]) admit direct native hashing and open-addressing probe
sequences.

Give `Map`/`Set` their own connector and native stencil family instead of routing
through the generic host-call path: an open-addressing table over `Value` slots, with a
guarded fast hash/compare stencil for the numeric-key and interned-string-key cases
(reusing the interning from [[11-string-representation]] so string-key comparison is a
pointer compare, not a byte compare) and a generic fallback hash/compare for object
keys and NaN/−0 edge cases. `get`/`set`/`has`/`delete`/`add` each compile to a stencil
that probes the table inline rather than crossing into a host call.

Acceptance: `Map`/`Set` correctness tests (including key-equality edge cases: `NaN`
key equality, `-0`/`+0` identity, `SameValueZero` semantics) pass; a numeric- or
interned-string-keyed `Map`/`Set` compiles `get`/`set`/`has` to inline probe stencils
with no host-call boundary crossing, verified by a call-count counter; alternating A/B
on `Map`/`Set`-using suites shows a measured gain over the prior host-call
implementation.
