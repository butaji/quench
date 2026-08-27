# L0–L4 execution profile

> **Integrity rule:** performance lanes measure the same Node-compatible VM;
> they are never benchmark dispatch. No lane may detect V8-v7/Octane source,
> call another engine, compare an expected score/checksum, or replace a
> workload with a benchmark-specific implementation. See
> [`benchmark-integrity.md`](benchmark-integrity.md).

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
  and packed get/set/miss counts split by miss reason, rejected array kind,
  and stale/non-array word. Bounded
  `owned_word_read_by_site` and `owned_word_read_by_op` rankings attribute
  retained/cloned word materialization independently from value decoding;
  `value_decode_other_by_op` resolves the residual decode `other` site.
- `l1`: reusable shape hits, counted-loop admission, leaf admission/rejection
  classes, and native kernel IDs with hits and deopts. These are observations;
  they must not become workload-specific dispatch.
- `l2`: handler count/share, main and leaf `Slow` gateways, the top eight
  compact opcodes for both dispatch paths, and `top_compact_sites`, the top 64
  code-id/PC/source-offset sites with a seven-opcode context window. Collection
  is exact up to 4,096 sites; `compact_site_dropped` must remain zero.
- `l3`: handler count/share, top eight slow operations, descriptor-object
  origins, the VM ops that materialize descriptor views, allocation origins,
  and RegExp `lastIndex` access paths.
- `l4`: host-call count and non-Function call targets.

`loop_shapes`, `function_call_shapes`, and `heap_lifecycle` remain beside
`lanes` in the snapshot.

## Measurement contracts (diagnostic only)

`quench-bench/profile-contracts.json` describes optional measurements for the
external benchmark harness. It never selects runtime behavior, and a contract
failure must not be “fixed” with a benchmark-specific path. Assert a real
traced/untraced run with:

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
measurements only; they are not semantic or score gates.

`vm_share_ppm` divides only L2 and L3 handlers. L0 operations and L1/L4 entries
overlap those handlers and therefore must not be presented as exclusive time.
Use the external harness and OS hardware counters for wall time, cycles,
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
from the measured binary, so symbols do not perturb timing/RSS evidence.
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
failed assertion names exactly one `<micro>.<metric>` key. Benchmark scores may
be recorded for comparison, but acceptance evidence is Node-compatibility
behavior and the ordinary VM path.

G1 drills also declare an `oracle.max_ratio`. Build the untraced measurement
binary, then compare each JS micro with the matching optimized Rust/C-fast
algorithm:

```sh
cargo build --profile bench-throughput -p quench-node
node tools/assert-l0-oracles.cjs
```

The harness reports median process time after one warm-up. This keeps startup,
parsing, lowering, and execution in the same G1 budget while the trace binary
continues to measure lane composition separately.
