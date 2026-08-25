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
- `l2`: handler count/share, main and leaf `Slow` gateways, the top eight
  compact opcodes for both dispatch paths, and `top_compact_sites`, the top 64
  code-id/PC/source-offset sites with a seven-opcode context window. Collection
  is exact up to 4,096 sites; `compact_site_dropped` must remain zero.
- `l3`: handler count/share, top eight slow operations, descriptor-object
  origins, allocation origins, and RegExp `lastIndex` access paths.
- `l4`: host-call count and non-Function call targets.

`loop_shapes`, `function_call_shapes`, and `heap_lifecycle` remain beside
`lanes` in the snapshot.

## Executable benchmark contracts

`quench-bench/profile-contracts.json` is the single declaration of the expected
execution profile for every V8-v7 benchmark. Assert a real traced/untraced run
with:

```sh
node tools/analyze-quench-bench.cjs deltablue \
  --assert-profile quench-bench/profile-contracts.json
```

Run the contract for every declared benchmark, or a named subset, with:

```sh
node tools/assert-quench-bench-profiles.cjs
node tools/assert-quench-bench-profiles.cjs deltablue richards
node tools/assert-quench-bench-profiles.cjs deltablue -- --timeout-ms 300000
```

The contract addresses any numeric field in the combined report by path and
supports inclusive `min` and `max` bounds. Exact architectural invariants use
the same value for both. Deterministic lane counters and normalized ratios are
the primary contract; Score remains an untraced end-to-end gate.

`vm_share_ppm` divides only L2 and L3 handlers. L0 operations and L1/L4 entries
overlap those handlers and therefore must not be presented as exclusive time.
Use untraced benchmark scores and OS hardware counters for wall time, cycles,
instructions, cache misses, and peak RSS; join them with this report by suite,
binary commit, profile, and run number.

On macOS, build a separate symbolized binary without execution tracing and
join sampled self-time into the same report:

```sh
CARGO_TARGET_DIR=target-sample RUSTFLAGS='-C debuginfo=1' cargo build \
  --profile bench-throughput -p quench-node
node tools/analyze-quench-bench.cjs deltablue --sample-seconds 3 \
  --sample-quench target-sample/bench-throughput/quench-node
```

`sample.top_self` contains the top 32 Quench symbols with sample counts and
normalized `share_ppm`. The sample binary is untraced but remains separate
from the stripped Score binary, so symbols do not perturb Score/RSS evidence.
If macOS `sample` is unavailable, `sample.available` is false and the other
measurements remain valid.

For native admission debugging, enable `QUENCH_DUMP_LOOP_SHAPES=1`. Counted
loop dumps include `per_iteration`, while `l1.counted_recognized`,
`counted_per_iteration_rejects`, `counted_attempts`, `counted_hits`, and
`counted_deopts` identify the exact boundary where a proven loop leaves L1.

## Lane micro contracts

Focused drills live in `tests/lanes/<id>.js` with one adjacent
`<id>.want.json`. The want file declares normalized measurements and minimum
traffic, so an unstressed or missing path fails rather than producing a false
zero. Run all drills or a named subset against the separate trace binary:

```sh
node tools/assert-lane-micro.cjs
node tools/assert-lane-micro.cjs u64-move getn-number call-fp
```

Each output line contains the measured numerator, denominator, and ratio. A
failed assertion names exactly one `<micro>.<metric>` key. Full V8-v7 Score on
the untraced binary remains the acceptance exam after a drill turns green.
