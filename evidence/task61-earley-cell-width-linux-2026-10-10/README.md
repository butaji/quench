# EarleyBoyer cell slot width reduction (2026-10-10)

## Change

The largest `Cell` alignment on the Linux production target came from the
inline `i128` epoch fields in `TemporalInstant` and `TemporalZonedDateTime`.
Those variants are absent from EarleyBoyer's completed heap census, while its
74,319 live cells occupy 143,360 reserved slots. Boxing only those two cold
numeric payloads reduced the production `Cell` and `Slot` layout from 96 bytes
with 16-byte alignment to 88 bytes with 8-byte alignment. The [compiler layout
output](layout.txt) records both layouts. This uses the actual runtime enum and
slot, independently measuring the V2 compact-cell probe rather than treating
its stand-in types as a runtime result.

The Rust Reference says the default Rust representation does not guarantee
the remaining enum layout, so these sizes are measurements for rustc 1.99.0
on this Linux target. Recheck the layout on the Stage B target before applying
the byte savings there. See the [Rust type layout
reference](https://doc.rust-lang.org/reference/type-layout.html#the-rust-representation).

## Correctness and measurement

The production build succeeded. A focused Node v24.19.0 `--harmony-temporal`
oracle matched baseline and candidate for Instant nanoseconds, ZonedDateTime
construction and arithmetic, DST behavior, rounding, conversions, and
comparisons. The [oracle source and outputs](temporal-box-oracle.js) are
preserved. EarleyBoyer's output matched in every alternating production pair.

Across 11 pairs against the `e4a874daf` baseline, the candidate's median Score
rose from 407 to 426 (paired median delta +19; 100,000-resample 95% interval
[+14, +22]). Median maximum RSS fell from 42,647,552 to 41,697,280 bytes
(paired median delta −991,232; interval [−1,081,344, −790,528]). Candidate
Score was higher and RSS lower in all 11 pairs. Median wall time fell by
991,075,064 ns. The [paired report](boxed-i128-paired-confirm.json), [raw
rows](boxed-i128-pairs-confirm.jsonl), [runner](paired-11.py), source patch,
and per-run output are preserved here.

This improves EarleyBoyer Score and RSS on Linux, but does not meet Stage B's
reference-engine leadership target and does not establish the result on M4 or
the other seven fixtures. The Temporal allocations are a small, cold-path
tradeoff that still needs cross-fixture qualification.

## Reproduction inputs

Baseline binary SHA-256:
`99ff70c9dfc139882a3c284481158b1ebac2da975641324fe7c2d5a46f31a1de`.

Candidate binary SHA-256:
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`.

Fixture SHA-256:
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

Node oracle SHA-256:
`0f2fa2955a80008a1bef93e549a21212b25c17b7be4f2b7a218625c28bb01ffb`.

See [host.txt](host.txt) for toolchain and host limits. Instrumentation was
disabled during timing and RSS measurements.
