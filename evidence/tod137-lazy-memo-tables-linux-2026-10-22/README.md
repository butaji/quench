# TOD-137: direct-mapped memo tables allocated on first record — kept

Host: Linux 6.18 x86_64 VM, 4 logical CPUs, 16 GB. Production profile `quench-node`
(plain, not BOLT), `QUENCH_MEMORY_POLICY=memory-first`, compile cache on.
Each run starts with the engine binary evicted from the page cache (`rss-exec --evict`). Pairs run in alternating order.
Medians are shown; `abquick.py` is the driver.

- Base: v2-gc at 8900e91e56.
- Candidate: `cand-gc-lazy-memo-tables`.

## Change

Eight fixed-size, direct-mapped memos were filled entry by entry when each VM was built:

- the shape slot and attribute memos
- the name-atom and string-atom memos
- the symbol load proofs and their site table
- the symbol write proofs and their site table
- private-site homes
- function-name shapes
- the enumerable-key memo

Every page of these tables was resident from start-up, whether or not the program used
private names, symbol keys or enumeration. They now go through one type,
`shape_memo::DirectMappedTable`, which allocates on the first record:

- A probe of an unallocated table answers "no entry".
- `clear` skips an unallocated table.
- The enumerable-key memo holds `Rc` values rather than `Copy` ones, so it does the same with an
  empty boxed slice.

## Counterfactual first

To check that the tables matter at all, a measurement-only branch (`meas-gc-memo-tables`)
shrank every table to 16 entries:

- Peak RSS fell 76–336 KiB.
- Some no-JIT scores fell up to 8%.

So the full-size tables are resident, and they earn their size once used.

## Inlining the allocation cost DeltaBlue `--jit`

With the allocation inlined into `set` (`OnceCell::get_or_init`), DeltaBlue `--jit` read:

| Runs | Base | Candidate | Change |
| --: | --: | --: | --: |
| 5 | 1140 | 936 | |
| 10 | 1214 | 1164 | |
| 20, alternating | 1217 | 1116 | −8% |

Keeping the shape memos eager removed both the loss and most of the RSS gain.
Making the allocation `#[cold] #[inline(never)]` restored the speed and kept the gain:
DeltaBlue `--jit` read 1195 against 1186 over 20 alternating runs.

## Result (10 runs per row, `ab-both-modes-10-runs.txt`)

Peak RSS in KiB.

| Fixture | no-JIT Δ | `--jit` Δ |
| -- | --: | --: |
| print1 | −132 | −4 |
| richards | −48 | −108 |
| deltablue | −176 | −108 |
| crypto | −254 | −216 |
| raytrace | −176 | −228 |
| regexp | −134 | −132 |
| earley-boyer | −146 | −118 |
| splay | −118 | −136 |

A/A noise floor, with the same binary twice:

- RSS within ±22 KiB.
- Scores within ±3%.

Scores are within that floor on every row, except Richards no-JIT in one 10-run set (−6%).
A 20-run alternating recheck read −2% for Richards and +5% for DeltaBlue.

Only print1 `--jit` stays within noise. Why it does was not investigated.

## Verification

`cargo test -p quench-runtime --lib`: 823 passed.
