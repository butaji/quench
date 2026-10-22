# TOD-167: MemoryFirst with a 2^13 young minimum on the V8-v7 board

Host: Linux x86_64 VM, 4 logical CPUs, 15 GB, kernel 6.18.

- **Binary:** one static + BOLT `quench-node` (`qb4`) from v2-gc `025fbcd4b`, which raises
  `MEMORY_FIRST_PACING`'s young minimum from 768 to 2^13. Both policies share the binary,
  selected with `QUENCH_MEMORY_POLICY`.
- **Board:** `quench-bench/v8-v7-rss.cjs`, 3 runs, evicted binaries; QuickJS-ng 0.10.1, Node
  22.22.0 and Bun 1.4.2 in both their modes.
- **Discarded run:** the first board on this binary started right after BOLT wrote it. It
  measured every Quench sample about 7.2 MB higher, with QuickJS, Node and Bun unchanged.
  The same binary measured normally minutes later. A rerun is recorded here; the cause, most
  likely the freshly written binary's page-cache folios, is still being checked.

## Max RSS: Quench's worst sample in both modes over the best other engine's (`summary.txt`)

| Fixture | Throughput | MemoryFirst 2^13 | MemoryFirst 768 (previous board) |
| -- | --: | --: | --: |
| richards | 1.35 | 0.89 | 0.82 |
| deltablue | 2.02 | 1.06 | 0.90 |
| crypto | 2.59 | 1.41 | 1.35 |
| raytrace | 1.95 | 0.93 | 0.83 |
| earley-boyer | 0.63 | 0.37 | 0.38 |
| regexp | 1.81 | 1.05 | 0.94 |
| splay | 0.53 | 0.51 | 0.51 |
| navier-stokes | 1.84 | 1.78 | 1.48 |

The best other engine is QuickJS everywhere except splay (Bun without JIT). The target is
0.80. MemoryFirst meets it on 2 fixtures and is within 0.06–0.13 of it on richards and
raytrace. Navier-stokes's remaining gap is `--jit` compile memory (TOD-110).

## Score: MemoryFirst 2^13 over Throughput, same binary (`scores.txt`)

| Mode | Geomean | At 768 (previous board) | Largest losses |
| -- | --: | --: | -- |
| No JIT | ×0.902 | ×0.777 | earley-boyer ×0.77 |
| `--jit` | ×0.879 | ×0.736 | deltablue ×0.61, earley-boyer ×0.79 |

## Against the other runtimes' Score (`scores.txt`)

- **No JIT:** Quench beats QuickJS everywhere under both policies, by 1.6× to 10×. Node
  `--jitless` beats Throughput on raytrace, earley-boyer and regexp, and Bun without JIT
  beats it on richards.
- **JIT:** Node and Bun score several times higher on every fixture except splay.
- Neither policy is 20% better than every runtime on Score, so the owner's target is out of
  reach for both on that axis. The policies differ only on max RSS.

## Recommendation

Make MemoryFirst with the 2^13 minimum the default, with Throughput as the opt-in:

- It is within ~10% of Throughput's Score, and at about half of its max RSS on six fixtures.
- Throughput uses 1.3–2.6× QuickJS's max RSS on six fixtures.

Before the switch, measure web servers. The default's doc comment records HTTP servers at
about half their throughput under the old memory-first table, and koa's max RSS rose by
1.1 MB at a 1024 minimum. That setup is not in this container.
