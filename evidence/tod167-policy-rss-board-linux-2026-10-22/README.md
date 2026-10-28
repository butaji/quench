# TOD-167: Throughput and MemoryFirst on the V8-v7 max-RSS board

Host: Linux x86_64 VM, 4 logical CPUs, 15 GB.

- **Binary:** one static + BOLT `quench-node` (`qb3`) from v2-gc `e69d784dd`, which includes
  v2-merged's memory policy (`19b3b0dc1`). Its BOLT profile predates that merge, so absolute
  values sit about 100–200 KiB above a freshly trained layout. Both policies share the binary.
- **Policy:** selected with `QUENCH_MEMORY_POLICY`.
- **Board:** `quench-bench/v8-v7-rss.cjs`, 3 runs, evicted binaries; QuickJS-ng 0.10.1, Node
  22.22.0 and Bun 1.4.2 in both their modes.

## Max RSS: Quench's worst sample in both modes over the best other engine's (`summary.txt`)

| Fixture | Throughput (default) | MemoryFirst |
| -- | --: | --: |
| richards | 1.32 | 0.82 |
| deltablue | 1.95 | 0.90 |
| crypto | 2.94 | 1.35 |
| raytrace | 1.94 | 0.83 |
| earley-boyer | 0.69 | 0.38 |
| regexp | 1.79 | 0.94 |
| splay | 0.54 | 0.51 |
| navier-stokes | 1.83 | 1.48 |

- The best other engine is QuickJS on every fixture except splay (Bun no-JIT). Node and Bun use
  5–100× more.
- Under Throughput, Quench uses 1.3–2.9× QuickJS's max RSS on six fixtures.

## Score: MemoryFirst over Throughput, same binary (`scores.txt`)

| Mode | Geomean | Largest losses |
| -- | --: | -- |
| No JIT | ×0.777 | raytrace ×0.44, earley-boyer ×0.51, regexp ×0.75 |
| `--jit` | ×0.736 | raytrace ×0.45, earley-boyer ×0.56, deltablue ×0.61 |

MemoryFirst's young minimum of 768 makes small heaps collect thousands of times, and each
collection's fixed cost is that difference (TOD-164).

## JIT mode

`--jit` raises navier-stokes max RSS to 7.8 MB under MemoryFirst, against 4.4 MB without JIT.
`rsssplit2.py` shows the cause: 2.3 MB of anonymous compiler working memory and 1.5 MB of
compiler code pages at the compile, freed afterwards.

## Conclusion

Neither table meets the owner's target of being 20% better than every other runtime alone:
- **Throughput** loses max RSS to QuickJS.
- **MemoryFirst** gives up a quarter of the speed.

The way to both is Throughput's speed at MemoryFirst's young size:
- **Per-collection fixed cost:** TOD-164 step 3, minor collections that skip unchanged roots
  and frames.
- **Young sizing between the two tables:** swept next.
