# Splay M4 refresh on the `ba3` runtime

Host: Mac16,10, Darwin arm64 T8132, 10 logical CPUs, 16 GiB; Rust 1.99.0. The runtime is from `ba3eee054`; the pinned production `quench-node` SHA-256 is `b1222e3d7f636d9d9863f90eeb1d5b63b7ce0f1fb8833657cd0818ecc9a5cd62`. The fixed-work runner SHA-256 is `f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`. Evidence was collected on 2026-10-11.

## Stock harness

`stock-harness-splay-11.json` records 11 rounds of the stock V8-v7 harness. All rounds were valid and outputs matched. The runner sets `qualification_ready` to `false`; treat this single-fixture refresh as a diagnostic, not as a qualification declaration.

| Engine | Median Score | Median maximum RSS |
| --- | ---: | ---: |
| Quench | 6,090 | 123,027,456 B |
| Node v26.10.0 `--jitless` | 14,139 | 424,296,448 B |
| Bun 1.4.2, JIT disabled | 9,624 | 61,472,768 B |
| QuickJS 2026-06-04 | 8,442 | 151,060,480 B |

Node is the stock-score leader and Bun is the lowest-RSS reference. The current stock-harness distances are therefore **2.32x speed** (14,139 / 6,090) and **2.00x RSS** (123,027,456 / 61,472,768). Quench has not won Splay.

## Fixed-work diagnostic

`four-engine-fixed-work-11.json` is a separate schema-4, 11-round diagnostic with 300 `run()` calls and paired `K=0` setup-only subtraction. All four engines have 11 valid and clean samples. Marginal cycles/run are Quench 5,269,142; Node 4,984,757; Bun 6,666,413; QuickJS 3,853,341. QuickJS is fastest on this metric, making Quench **1.37x** the fixed-work fastest reference; against Node alone it is 1.06x. The fixed-work reference ranking must not be substituted for the stock Score ranking.

Fixed-work maximum RSS was Quench 122,732,544 B, Bun 60,424,192 B, Node 256,917,504 B, and QuickJS 151,076,864 B. This is diagnostic and distinct from the stock-harness medians above.

The stock run recorded one-minute host load averages from 2.60 to 3.77 across rounds. Raw per-round load, process metrics, executable hashes, and command provenance are preserved in the JSON files. The source revision field names this evidence-only commit; its runtime source is unchanged from `ba3eee054` (`e2445fc1f` changed only evidence files).
