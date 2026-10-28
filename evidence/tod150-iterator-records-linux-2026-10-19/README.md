# TOD-150: iterators and generators as a record family

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB, no cgroup CPU quota; rustc 1.99.1.
Release builds; base `c149` = v2-gc-tod149 db74913b, candidate `c150` = 0b9852f4.

| Test | Score, paired | Peak RSS |
| -- | -- | -- |
| async-fs | ×1.039, 4/5 | 21.6 → 21.3 MB |
| doxbee-promise | ×1.074, 3/5 | 65.9 → 65.8 MB |
| doxbee-async | ×0.975, 2/5 | 54.4 → 54.4 MB |
| V8-v7 | geomean ×1.027 | equal or −0.1 MB on each |
| npm top 300 | — | geomean ×0.9972, none >5% worse |

## web-ssr: a collection-timing knife edge, not an iterator cost

web-ssr peak RSS read 73.1 → 84.4 MB (RssAnon +11.6 MB, deterministic). Analysis:

- A switch disabling iterator records in the same binary restores 72.7 MB, so the family
  changes something; its own space touches 3 blocks (~250 KB, `iterator_record_reserved_bytes`).
- Live counts per collection are the same (QUENCH_GC_TRACE: max live 40,432 vs 40,336); the
  candidate allocates 6.5% more weight per collection (1,199 → 1,277 mean), so collections land
  at different safepoints.
- Young budget sweep (gc-tuning builds of both, VmHWM KiB): 1/16 73,456 / 73,584; 1/9
  74,356 / 74,260; 1/7 74,688 / 75,168; 9/70 74,232 / 74,772; 1/4 76,532 / 76,872; only the
  production 1/8 gives 73,356 / 84,736.

web-ssr builds large strings; its peak depends on how many dead ones a collection happens to
find. That sensitivity is the controller's (TOD-151), not this family's.

Runtime lib 744/744 (minor verifier on). A collection's iterator registry no longer reads a
collected iterator's identity (`move_collection_iterators`), which may name a free record.
