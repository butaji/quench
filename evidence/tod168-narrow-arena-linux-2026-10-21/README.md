# TOD-168: narrow 32-bit property arena vectors — rejected

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. Release `quench-node`, both binaries run with
`QUENCH_COMPILE_CACHE=`.

- Base `b168`: v2-gc at 0299d8ee2 (TOD-166 plus the four-value arena minimum).
- Candidate `c168`: `cand-gc-narrow-arena` (format 103).

## Change

- `ValueArena` keeps two lanes:
  - `values` (`Value`)
  - `narrow` (`u32`, `Value::to_narrow`)

  Each has its own free lists.
- A range lives in the narrow lane while every value has a narrow form.
- A store or push of any other value moves the range to the wide lane, and the owner
  stores the moved vector again.
- Compaction slides each lane on its own.

## Result: rejected

Rule (TOD-168):
- RSS geomean ≤ ×0.99 on JetStream or V8-v7, and no suite's RSS above ×1.005.
- No suite's speed below ×0.99.
- npm ≤ ×1.0 with no new failures.

| Suite | Pairs | Speed | RSS |
| -- | --: | -- | -- |
| V8-v7 | 10 | **×0.973** (richards ×0.880, 1/10) | ×1.004 |
| JetStream (12 tests) | 5 | **×0.983** | ×0.972 (hash-map 24.4 → 21.4 MB, acorn 37.7 → 35.3, espree 49.0 → 46.4, source-map 58.9 → 56.5) |
| Frameworks | 5 | ×1.009 | ×0.993 |
| npm top 300 | 1 | — | ×0.9945; execa fails on both binaries |

quench-runtime lib: 792/792.

The memory gain is real, but the speed rule fails on V8-v7 and JetStream. Callgrind on
symbol builds puts richards at +4.6% instructions (1157M → 1210M). The cost is in field
reads (`lane_get_field_own`, `lane_get_field_sink_own`) and this-field stores: every
arena access now branches on the lane and decodes or encodes the value.
