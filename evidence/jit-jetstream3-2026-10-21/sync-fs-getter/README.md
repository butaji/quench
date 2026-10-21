# JetStream sync-fs cached getter screen

`File.get data()` compiles to three bytecodes: a return-marked `GetField(this)`
followed by an unreachable `LoadConst` fallback and an ordinary `Return`. The
cached-this field-return matcher previously admitted only a four-bytecode
shape. It now also admits the observed three-bytecode shape, preserving the
same fixed-this check, own data-property cache guard, and interpreter fallback.

A validated one-round iteration-profile diagnostic recorded 36,951,119 fast
returns out of 36,951,120 getter entries; the first field-cache miss fell back
to the interpreter. The short local getter workload also used the guarded path
999 times out of 1,000 and completed with equal results.

Three clean production samples per binary give a directional improvement.
The pre-change control scores were 4.01, 1.83, and 2.59 (median 2.59); the
candidate scores were 2.65, 3.81, and 2.86 (median 2.86). Median Quench wall
time fell from 123.38 s to 112.25 s. Median peak RSS was effectively flat at
72.22 MiB for control and 72.26 MiB for candidate. The third control/candidate
pair was 2.59 / 123.38 s versus 2.86 / 112.25 s. Bun and Node moved
substantially between rounds, and the first two samples were grouped by
binary, so this remains a small directional screen rather than qualification
evidence. Quench still trails both peers by a wide margin.

## Reports

- `../sync-fs-getter-diagnostic-final-1round.json`: final JIT coverage
  diagnostic, iteration profile.
- `../sync-fs-production-control-1round.json`: production control before the
  three-bytecode matcher extension.
- `../sync-fs-production-getter-1round.json`: production candidate.
- `../sync-fs-production-control-2round.json` and
  `../sync-fs-production-control-pair3.json`: three pre-change control samples.
- `../sync-fs-production-getter-2round.json` and
  `../sync-fs-production-getter-pair3.json`: three candidate samples.

The earlier `sync-fs-getter-diagnostic-iter-*` reports document rejected
matcher shapes encountered while diagnosing the exact bytecode layout.
