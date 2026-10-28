# Guarded multi-field JIT inputs

## Change

The general numeric JS compiler now accepts up to four cached data-property
reads from `this` or parameter zero in an otherwise pure numeric region. Each
read is represented by its bytecode PC, cache site, and receiver kind. On every
entry, the VM probes the live receiver and appends only Number values to the
native ABI. A missing cache hit, accessor, proxy, non-number, or non-fixed
`this` leaves the original frame unchanged and executes from PC 0 in the lane.
Captures remain fresh inputs; generated code stores no guest object references.

The test `js_jit_lowers_multiple_guarded_this_field_reads_into_arithmetic`
checks two fields, native entries, an accessor that runs once after fallback,
and a non-number result that is interpreted. The existing parameter-zero test
also passes using the generalized field-input metadata.

## Correctness and coverage

- `quench-runtime --lib`: 799 passed, 0 failed.
- `cargo build --locked --profile iteration -p quench-node`: passed.
- Host: Linux x86_64; iteration profile, opt-level 0, 16 codegen units, two
  Cargo jobs, serialized LLVM backend, debug info disabled.
- Stock V8-v7 Richards completed with assertions enabled under
  `QUENCH_JS_JIT_STATS=1`. Its `isHeldOrSuspended` function (function 45) had
  32 entries and zero native entries. It still rejects because its fields are
  combined with numeric captures and a branch. This step therefore adds no
  measured Richards coverage.
- The JetStream 3 `typescript-lib` and `async-fs` diagnostics did not complete
  within the 600-second per-engine timeout on the iteration binary. The reports
  are marked incomplete and contain no valid score samples. The `richards.js`
  attempt was invalid because that workload belongs to V8-v7, not JetStream 3;
  its report is not retained here.

No matched score or RSS improvement is established. The feature is retained as
a general compiler capability and testable foundation for broader guarded
regions; it is not a performance win or qualification.

Raw valid Richards stdout/stderr and incomplete JetStream reports are beside
this file. The source runner and binary hashes are recorded in those reports.
