# Architectural evidence without a second VM

Status: research recommendation. Instrumentation is optional and must not change
JavaScript semantics, object lifetime or native admission decisions.

## Evidence and limits

At snapshot `06e71d3f4`, the first three-second Richards trace-enabled sample
contains 390 collapsed top-of-stack samples in SipHash and explicit counter/site
hashing stacks. The same output includes waiting threads and loader activity:
these counts are not percentages of VM execution and not a whole-run profile.
Trace-disabled sampling is being collected separately. Its Richards sample has
81 collapsed top-of-stack samples in `required_register_count`, alongside
dispatch/property work; hashing no longer heads the executing-code list. These
adaptive workloads ran under shared-host contention, so sample counts and elapsed
times are not a controlled estimate of tracing overhead. Never conclude that
production hashing is the main Richards bottleneck from this instrumented run.

`execution_trace.rs` already generates event enums and some lifecycle hooks,
uses dense opcode counters and bounds several site maps. Preserve this foundation.
Other observations use string-keyed maps, operand-pair maps, and repeated function
fingerprinting. `function_call_shape` computes a recursive code fingerprint;
`hash_code_facts` also formats constants. Inspect actual call frequency before
assigning cost. Static code identity is not dynamic uncertainty.

Historical lifecycle counts additionally mix constructor and physical-wrapper
populations. They cannot establish live heap by subtraction. Revalidate coverage
after ownership changes; this is an accounting-contract issue, not proof of a leak.

## Recommended shared model

1. Declare event identity, units, scope, population and aggregation once in Rust
   data/macros. Derive wire names, counter indices, schema and validation.
2. Use existing immutable code/site IDs on the hot path. Cache or derive expensive
   fingerprints at code finalization or report serialization, with explicit code
   lifetime and reuse rules. Do not retain executable owners merely for profiling.
3. Fixed event kinds use dense counters. Optional detailed site/pair collection
   has explicit entry/byte budgets and reports dropped observations. Avoid a full
   quadratic opcode-pair table when sparse bounded collection is smaller.
4. Keep cheap totals separate from detailed sampling. Admission to a bounded map
   is not unbiased sampling: first-seen sites can exclude later hot code. Label
   that policy and its omissions; offer targeted IDs or a documented sampling
   method where needed. Missing is not zero.
5. Distinguish JS semantic creation, physical header/backing allocation, resize,
   Rc retain/release and final destruction. A live gauge needs equal populations,
   opening state and report boundaries. Count code/cache/retired-live bytes too.
6. Reports and symbolization happen outside hot execution. Effects stay at this
   boundary; avoid string construction, formatting and repeated metadata walks
   merely to increment a known event.

This does not require an event-sourced runtime, universal telemetry framework or
another ownership registry. Reuse the operation, allocation and code contracts.
Prefer bounded per-thread observations with safe teardown; justify atomics only
where cross-thread aggregation or the actual ownership model requires them.

## Acceptance

- Compare tracing disabled, cheap counters and detailed sites on identical fixed
  work, separately from scored runs. Record both time and memory perturbation;
  choose budgets from measurements, not an invented zero-overhead guarantee.
- Test declared events, clone/allocation distinction, resize, code retirement,
  capacity exhaustion, site-ID reuse and teardown. Formatting cannot run in the
  event increment path; omitted events remain explicit in output.
- CPU samples exclude neither startup nor idle threads silently. Attribute phases
  and thread populations where available. Initial-window samples are incomplete
  evidence for long-running workloads. Physical footprint is not RSS.
- Benchmarks remain architecture-neutral. Their output should distinguish missing
  observations, semantic failures and measured weaknesses without requiring a
  specific collector, JIT, stencil family or implementation strategy.

075 owns required native evidence contracts; 073 owns full-corpus profiling and
perturbation validation. Do not expand the infrastructure gate into an unrelated
telemetry rewrite. Prioritize only changes necessary for trustworthy decisions.
