# L0–L4 execution profile

Build the opt-in trace binary separately so counters cannot affect the scored
artifact:

```sh
CARGO_TARGET_DIR=target-exec-trace cargo build \
  --profile bench-throughput -p quench-node --features execution-trace
QUENCH_EXEC_TRACE=1 target-exec-trace/bench-throughput/quench-node program.js \
  2>trace.json
sed 's/^QUENCH_EXEC_TRACE //' trace.json | jq '.lanes, .heap_lifecycle'
```

`lanes` separates exclusive VM retirement from nested substrate work:

- `l0` reports representation pressure: word reads and copies, `Value`
  materialization, property cache outcomes, and packed-array accesses. Heap and
  Environment allocation counts remain in `heap_lifecycle`.
- `l1` reports native admission and work. A proven counted loop is healthy only
  when hits match its iterations and deopts are zero.
- `l2.handlers` counts compact and proven-leaf semantic handlers, excluding
  `Slow` gateways.
- `l3.handlers` counts slow semantic handlers plus cold `Slow` operations still
  executed by the leaf lane.
- `l4.host_calls` counts non-JavaScript call targets. Host work is nested under
  a call handler, so it is intentionally not added to the L2/L3 share.

`vm_share_ppm` divides only L2 and L3 handlers. L0 operations and L1/L4 entries
overlap those handlers and therefore must not be presented as exclusive time.
Use untraced benchmark scores and OS hardware counters for wall time, cycles,
instructions, cache misses, and peak RSS; join them with this report by suite,
binary commit, profile, and run number.

For native admission debugging, enable `QUENCH_DUMP_LOOP_SHAPES=1`. Counted
loop dumps include `per_iteration`, while `l1.counted_recognized`,
`counted_per_iteration_rejects`, `counted_attempts`, `counted_hits`, and
`counted_deopts` identify the exact boundary where a proven loop leaves L1.
