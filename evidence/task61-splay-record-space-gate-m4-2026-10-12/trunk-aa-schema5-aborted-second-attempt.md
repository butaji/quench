# Aborted second M4 same-binary calibration

This attempt is invalid for calibration and must not be used in a candidate
decision. The runner was stopped after both completed fixtures had zero clean
pairs; the remaining fixtures were not run.

## Provenance

- Platform: arm64 macOS, Mac16,10, 10 logical CPUs.
- Runner: schema-5 `quench-bench`, SHA-256
  `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Baseline and peer were the same production binary, SHA-256
  `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`.
- Candidate binary remains pinned separately at SHA-256
  `f8739affbfee468f88aad0546a6ea1de1aa4fb706024665c163db6c94d2ab180`;
  no candidate A/B was run.
- Command:
  `/Users/admin/Code/GitHub/quench-v2/target/pinned/bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109/quench-bench --all --fixed-work --runs 11 --quench <trunk> --quench-peer <same-trunk> --out trunk-aa-schema5-11.json`.
- Preflight at 2026-10-12 00:59:30 UTC: load averages 3.10 / 10.28 / 9.33.
  The one-minute load was just below the prior 0.32-per-CPU screening threshold,
  but the five- and fifteen-minute averages were elevated.
- The runner ran for approximately 4m34s. It was interrupted with SIGINT after
  the first two fixtures both reported 11 valid rounds and 0 clean pairs. It
  exited 130. No JSON report was written.
- At 01:04:34 UTC, load was 9.77 / 15.58 / 12.66.

## Partial results

| Fixture | Valid rounds | Clean pairs | Work per benchmark |
| -- | --: | --: | --: |
| Crypto | 11/11 | 0/11 | 4 iterations |
| DeltaBlue | 11/11 | 0/11 | 40 iterations |
| EarleyBoyer onward | not run | not run | — |

This is the second unusable same-binary A/A attempt. In this attempt, the
one-minute preflight alone did not predict a clean campaign: its value was
3.10 on a 10-CPU host while the five-minute average was 10.28. Treat that as
evidence to refine the start condition, not as a candidate or performance
result. The candidate gate remains pending.
