# 120 — Typed numeric/dense region IR

Status: complete

Introduce one immutable, quoted representation for candidate numeric/dense regions.
It is derived solely from `DynCode` control flow and def-use facts and stays as data
until rewrite and linking finish. Its nodes describe numeric values, fixed local slots,
dense element reads/writes, pure arithmetic, control edges, and explicit exits; source
names, benchmark identity, runtime heat, and machine bytes are not inputs.

The representation is hierarchical: operation sequences form blocks, blocks form loop
regions, and regions form functions. Every level implements the same typed
`Region<In, Out>` categorical connector and uses a normalized flat sequence as its free
monoid. Identity, associativity, labels, symbolic edges, effects, and guard obligations
are derived projections of that single quoted value—not parallel mutable tables.

All positional values, minimum region sizes, and bounded capacities must be named
constants. Selection thresholds are cost-policy data and cannot appear as unexplained
literals in match logic.

Acceptance: law tests cover identity/associativity and normalize differently grouped
forms identically; def-use tests reject live-value, alias, exceptional, and unsupported
effect gaps; compile-time tracing reports eligible V8v7 regions without affecting
selection or execution; no source-shaped patterns exist.

Actual implementation:

- `src/numeric_region/model.rs` defines `Region<In, Out>`, typed composition,
  per-context identity, normalized flat `Seq` values, traced loop nodes, operations,
  labels, effects, and guard requirements. These are projections from one immutable
  quoted value rather than independently maintained mutable tables.
- `src/numeric_region/analysis.rs` discovers natural bytecode backedges and derives
  numeric/dense regions solely from control-flow and def-use facts. It rejects
  unsupported effects, nested loops, external entries, multiple exits, escaping
  temporaries, and inconsistent guard types.
- Quoting remains cold. Normal execution does not retain a region tree in `DynJitCode`;
  explicit `DEEGEN_NUMERIC_REGION_TRACE=1` is the diagnostic edge that materializes and
  reports it. Task 121 may materialize a selected quote while linking, but must consume
  or discard it rather than adding persistent per-function metadata.
- All selection and positional constants are named. No benchmark or source identity is
  an analysis input.

Evidence:

- 52 release tests pass, including category identity/associativity normalization,
  dense-loop guard derivation, escaping-temporary rejection, exception rejection,
  external-entry rejection, and conflicting-type rejection.
- `reports/numeric-region-eligibility-summary.txt` records 31 general regions in the
  full V8v7 workload: 15 in Crypto and 16 in Navier-Stokes, spanning 12 to 132
  bytecodes. The rejection histogram is retained in the same artifact.
- `reports/typed-numeric-region-edge-smoke.jsonl` completes every V8v7 suite with the
  edge-only binary. Saved binary SHA-256:
  `14a342468ac4e46bd49a29f795706473a0808b6f58a484b21ae03ca34f82e341`.

Rejected representation:

- Keeping `Box<[QuotedLoop]>` inside every linked `DynJitCode` made a six-pair Crypto
  A/B regress from 1037.5 to 924 (-10.94%); see
  `reports/typed-numeric-region-crypto-ab-6/comparison.txt`. The quoted tree was removed
  from the runtime image.
- The retained edge-only implementation measured 1030 versus 1014 (-1.55%) in the same
  focused six-pair protocol; see
  `reports/typed-numeric-region-edge-crypto-ab-6/comparison.txt`. Task 120 is an IR and
  analysis prerequisite, not an accepted speed optimization; Tasks 121-123 must repay
  this cost and are measured against the accepted Task 119 baseline.
