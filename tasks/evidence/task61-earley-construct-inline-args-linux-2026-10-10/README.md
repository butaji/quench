# EarleyBoyer inline constructor arguments candidate (2026-10-10)

## Decision

Rejected as a Stage B candidate. Passing register-backed constructor
arguments as a borrowed slice instead of copying them into a `Vec` reliably
reduced maximum RSS, but did not establish a score improvement. The production
change was reverted; the candidate patch and raw runs are retained here.

## Paired production measurement

- Fixture: pinned materialized EarleyBoyer, SHA-256
  `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
- Runtime: Rust 1.99.0, Linux x86_64, Xeon Platinum 8573C, 4-CPU quota.
- 11 alternating baseline/candidate pairs. Process maximum RSS was recorded
  from `wait4`; each pair's semantic output matched.
- Baseline/candidate median Score: 229 / 211. Paired median score delta:
-5 points, 100,000-resample bootstrap 95% interval [-46, 21] (Python
`random.Random(20261010)`). Score improved in 5/11 pairs.
- Baseline/candidate median maximum RSS: 41,816,064 / 41,279,488 bytes.
Paired median RSS delta: -536,576 bytes, 100,000-resample bootstrap 95%
interval [-663,552, -405,504] (Python `random.Random(20261011)`). RSS fell in
11/11 pairs.
- Baseline binary SHA-256:
  `3833b880e38828f9941c7703de506b9ec9290f25dce230c1c60b823e6b01c431`.
- Candidate binary SHA-256:
  `14a91e643ac855c3e3fae0ca410722954358e8011d835e844515cbfb89c74a19`.

Because the score interval includes both material loss and gain, the candidate
does not meet the required joint score/RSS gate. The result is not counted as
a score win. See `pairs.jsonl`, `candidate.patch`, and the Node semantic oracle
in this directory.
