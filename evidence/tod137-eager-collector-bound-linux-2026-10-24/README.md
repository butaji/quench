# How much lower could any collector pacing take the MemoryFirst peaks?

The question: is the collector at its floor, or does garbage still set the peak? The probe runs
one `gc-tuning` binary (v2-gc `12475d5020`, static, unbolted) at today's MemoryFirst pacing,
and at the most eager pacing the knobs allow (`eager.py`):
- a young collection every 64 allocation units;
- full collections at 1/64 old growth;
- an allocation cap of 1/64.

Each MemoryFirst collection also releases read-only and free malloc pages. A temporary
experiment switch (`QX_NORELEASE`, not committed) skips those releases, which separates the
two effects (`eager3.py`).

Host: Linux 6.18 x86_64 VM, 4 CPUs. Binaries evicted before each run, compile cache warm,
medians of 5 (`eager-16-rows.txt`) and 7 (`eager-release-split.txt`) alternating runs.

## Eager against default, releases on (16 rows)

The eager collector lowers max RSS by 380–640 KiB on Richards, DeltaBlue, Crypto, RayTrace and
RegExp without JIT. Scores fall 15–94% (RayTrace 2127 → 127). NavierStokes and Splay move by
−130 KiB or less, and EarleyBoyer rises by +250 KiB.

## Split: garbage against page releases (no JIT, KiB, against default)

| Fixture | Eager | Releases off | Eager with releases off, against releases off = **garbage headroom** | Its score cost |
| -- | --: | --: | --: | --: |
| Richards | −220 | +768 | **−256** | −5% |
| Crypto | −316 | +236 | **−96** | −24% |
| RayTrace | −260 | +552 | **−136** | −79% |
| RegExp | −520 | +384 | **−432** | −45% |
| DeltaBlue | −432 | +300 | **+84** | −60% |

Reading:
- On Crypto, RayTrace and DeltaBlue, no pacing can remove more than about 100–140 KiB of
  garbage from the peak, even at the cost of most of the score. The collector is at its
  floor there. Most of what eager collection gains on those rows comes from releasing code
  pages more often; §8.2 of the design doc already prices continuous code release at about
  Score ×0.87.
- RegExp has about 430 KiB and Richards about 260 KiB of garbage still resident at the peak.
  Removing it with plain pacing costs 5–45% of the score, so it is a candidate for a cheaper
  mechanism, not for more frequent collection.
- The page releases at collection boundaries are worth 236–768 KiB per row on their own.
