# Numeric hash-map entry-loop experiment

This experiment adds a guarded entry fast path for the 23-instruction
`_findKeyEntry` loop in JetStream's `hash-map` fixture. It runs only for numeric
arguments, a fixed receiver, cached data-field reads, an own dense or sparse
array element, an owned/shared lexical capture, and an already-compiled
same-program numeric `equals` function. It polls interrupts, bounds each chain
to 256 entries, returns at the ordinary bytecode `Return`, and leaves the frame
at PC 0 on every guard miss. It is now disabled by default; set
`QUENCH_JS_JIT_NUMERIC_FIND=1` to exercise it.

## Diagnostic coverage

`hash-map-diagnostic-1round.json` validated the fixture and counted 53,999,968
fast returns across 54,000,000 `_findKeyEntry` entries. One initial cache miss
fell back to the interpreter. Diagnostic timing is excluded from score
comparison.

## Clean one-round screen

`hash-map-score-1round.json` is a non-diagnostic one-round run:

| Runtime | Score | Peak RSS | Wall time |
| --- | ---: | ---: | ---: |
| Quench JIT | 3.61 | 74.07 MiB | 125.32 s |
| Bun | 108.18 | 99.03 MiB | 1.75 s |
| Node | 124.44 | 159.59 MiB | 1.79 s |

This screen shows lower Quench RSS, but no score win. It is not a matched
multi-round comparison, and earlier one-round samples varied substantially;
the result does not qualify the JIT against JetStream 3.0. Keep this as
coverage evidence, not as a performance claim.

## Matcher-cache follow-up

The bytecode matcher originally ran at every entry. `ProgramStore` now keeps
the last positive `(program, function)` match, so intervening negative matches
do not evict it. The cached diagnostic again validated output and counted
53,999,968 fast returns out of 54 million entries.

| Run | Quench score | Peak RSS | Wall time | Bun score | Node score |
| --- | ---: | ---: | ---: | ---: | ---: |
| Matcher uncached | 3.61 | 74.07 MiB | 125.32 s | 108.18 | 124.44 |
| Matcher cached | 4.52 | 74.17 MiB | 111.36 s | 175.49 | 155.13 |

The later sample is directionally faster, but the peer scores shifted too, so
these one-round runs are not a matched comparison. They do not establish a
score win. The cached diagnostic and screen are in
`hash-map-cached-diagnostic-1round.json` and
`hash-map-cached-score-1round.json`.

## Interleaved same-binary screen

The switch was then measured in three interleaved one-round pairs with the
same production executable and benchmark command. All six outputs validated.

| Pair | Enabled score / time | Disabled score / time | RSS enabled / disabled |
| --- | ---: | ---: | ---: |
| 1 | 3.16 / 126.62 s | 4.98 / 105.48 s | 74.12 / 74.00 MiB |
| 2 | 3.84 / 117.55 s | 4.60 / 110.01 s | 74.10 / 74.11 MiB |
| 3 | 2.80 / 127.91 s | 4.09 / 114.85 s | 74.00 / 74.07 MiB |

The disabled arm was faster in all three pairs, by about 11% on the geometric
mean of wall-time ratios. RSS was effectively unchanged. This rejects the path
as a default optimization despite its high coverage; it remains available only
as an opt-in research arm. Each raw report is `hash-map-pair-<arm>.json`.

The changed default was verified with `hash-map-default-off-1round.json`, which
passed fixture validation and scored 4.03 at 74.10 MiB / 121.23 s. The earlier
sequential three-round arms are retained as
`hash-map-ab-on-3round.json` and `hash-map-ab-off-3round.json`; their order was
confounded, so the interleaved pairs above are the more useful comparison.
