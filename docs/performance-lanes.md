# L0–L4 execution profile

Build the opt-in trace binary separately so counters cannot affect the scored
artifact:

```sh
CARGO_TARGET_DIR=target-exec-trace cargo build \
  --profile bench-throughput -p quench-node --features execution-trace
QUENCH_EXEC_TRACE=1 target-exec-trace/bench-throughput/quench-node program.js \
  2>trace.json
sed 's/^QUENCH_EXEC_TRACE //' trace.json | jq \
  '.lanes, .heap_lifecycle, .function_call_shapes[:8], .loop_shapes[:8]'
```

Schema 5 `lanes` contains:

- `l0`: `word_reads.{fixed,local,register,owned}`, `word_copies`,
  `value_decode`, `value_decode_by_site`, property hit/miss and payload kinds,
  and packed get/set/miss counts split by miss reason. The bounded
  `value_decode_other_by_op` ranking resolves the residual `other` site.
- `l1`: shape hits, crypto hits/direct iterations, counted-loop admission,
  leaf admission/rejection classes, and the top native kernel IDs with hits and
  deopts.
- `l2`: handler count/share, main and leaf `Slow` gateways, and the top eight
  compact opcodes for both dispatch paths.
- `l3`: handler count/share, top eight slow operations, descriptor-object
  origins, allocation origins, and RegExp `lastIndex` access paths.
- `l4`: host-call count and non-Function call targets.

`loop_shapes`, `function_call_shapes`, and `heap_lifecycle` remain beside
`lanes` in the snapshot.

`vm_share_ppm` divides only L2 and L3 handlers. L0 operations and L1/L4 entries
overlap those handlers and therefore must not be presented as exclusive time.
Use untraced benchmark scores and OS hardware counters for wall time, cycles,
instructions, cache misses, and peak RSS; join them with this report by suite,
binary commit, profile, and run number.

For native admission debugging, enable `QUENCH_DUMP_LOOP_SHAPES=1`. Counted
loop dumps include `per_iteration`, while `l1.counted_recognized`,
`counted_per_iteration_rejects`, `counted_attempts`, `counted_hits`, and
`counted_deopts` identify the exact boundary where a proven loop leaves L1.
