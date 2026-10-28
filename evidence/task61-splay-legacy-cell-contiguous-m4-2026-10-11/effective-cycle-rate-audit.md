# Effective cycle-rate audit — M4

The schema-4 fixed-work detector missed rounds whose retired cycles per wall
second were far below the same binary's ordinary work-sample rate. The proposed
schema-5 detector compares each **work** sample's `cycles / wall_ns` with the
75th-percentile reference for the same executable SHA across work rounds, and
marks ratios below 0.90 as contended. Setup-only samples deliberately skip this
check: their startup-dominated A/A distribution is much wider and would reject
valid K=0 baselines.

## Calibration from saved A/A reports

| A/A campaign | Work samples | Work reference p75 | Work minimum | Old-clean work below 0.90× p75 | Setup samples below 0.90× p75 |
|---|---:|---:|---:|---:|---:|
| trunk A/A (`db950f16`) | 176 | 4.000 GHz | 3.944 GHz | 0 | 85 / 176 |
| current trunk A/A (`ceeca73f`) | 176 | 3.987 GHz | 3.786 GHz | 0 | 83 / 176 |
| holder-corrected A/A (`696ffb4b`) | 176 | 3.972 GHz | 2.080 GHz | 0; six low samples were already marked contended | 85 / 176 old-clean |
| prior-candidate A/A (`6846ff40`) | 176 | 3.951 GHz | 1.176 GHz | 33 old-clean samples newly rejected | 104 / 176 |

The threshold preserves every previously clean work sample in the two clean
trunk controls and the holder-corrected control. It catches 33 false-clean work
samples in the prior-candidate A/A. Applying it to setup-only samples would
instead reject 83–85 of 176 samples even in the clean controls, so it is only a
work-sample check.

## Reclassification of the saved screens

The Crypto candidate's round 0 was marked clean by schema 4 despite a 2.131 GHz
effective rate, versus a 3.952 GHz median for its other rounds (ratio 0.538).
The work-only schema-5 rule would leave 10/11 Crypto pairs clean. It would leave
11/11 NavierStokes pairs clean, 9/11 EarleyBoyer pairs clean and 10/11 RegExp
pairs clean. The saved eight-fixture prior-vs-contiguous run remains 11/11 on
all fixtures under this check.

These are offline reclassifications of saved schema-4 samples, not new timing
results. The fixed-work runner and guard source now emit/require schema 5, and
the guard checkpoint schema is 2 so it cannot resume with old clean labels.
After this harness change, rerun any comparison used for a final decision.
Machine-readable inputs and reclassifications are in
`effective-cycle-rate-audit.json`.
