# Aborted M4 same-binary baseline calibration

This attempt does not provide a usable A/A baseline and must not be used for a
candidate decision. The fixed-work runner was stopped after external workload
contended with the host; no final JSON report was written.

## Provenance

- Platform: arm64 macOS, `Mac16,10`, 10 logical CPUs.
- Source checkout: `cfdc12f9c816717128ae914dd2eae728cc642bf3`.
- Runner: `quench-bench` schema 5, SHA-256
  `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Both measured paths were the same production baseline binary, SHA-256
  `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`.
- Requested campaign: all eight V8-v7 fixtures, 11 fixed-work rounds, baseline
  against itself, with matched setup-only samples.
- It started at 2026-10-12 00:08 UTC with load averages 1.70 / 4.00 / 9.26.
  At 00:13 UTC, host load had risen to 26.48 / 16.60 / 13.16 while other
  projects' Deno/Bun test workers were active.
- The runner had reported Richards at 11/11 valid but 0/11 clean pairs. The
  campaign was terminated while its Splay sample was active (exit 143). No
  candidate A/B measurement was started.

## Partial runner output

| Fixture | Valid rounds | Clean pairs |
| -- | --: | --: |
| Crypto | 11/11 | 10/11 |
| DeltaBlue | 11/11 | 11/11 |
| EarleyBoyer | 11/11 | 11/11 |
| NavierStokes | 11/11 | 11/11 |
| RayTrace | 11/11 | 11/11 |
| RegExp | 11/11 | 10/11 |
| Richards | 11/11 | 0/11 |
| Splay | interrupted | interrupted |

The next measurement is a fresh A/A campaign after the host clears. Do not
resume from these partial rounds.
