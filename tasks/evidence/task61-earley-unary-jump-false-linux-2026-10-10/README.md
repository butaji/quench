# EarleyBoyer unary-branch fusion

## Change and decision

The V2-merge profile recorded 15,935,839 adjacent `Unary → JumpFalse`
executions. A liveness-guarded compiler recipe now fuses this pair into
`JumpUnaryFalse`; the VM evaluates the original unary operation exactly once,
tests its truthiness, and discards its result only when the register is dead on
every successor. The opcode preserves the unary operator, source operand,
branch target, and throwing/control effects.

The focused recipe and liveness tests passed. Production binaries were checked
against Node v24.19.0 on unary plus/minus, logical not, bitwise not, `typeof`,
`void`, `delete`, coercion order, and thrown-object identity. Node output matched
both binaries. EarleyBoyer benchmark output matched in all 11 alternating
production pairs.

Against baseline SHA-256
`3b305f12f60a7eb6024b305204a3bc07aea965f34bbeeda8e1885f02df61c32b`:

- Score medians: 398 baseline, 404 candidate. Paired median delta +3 points;
  95% bootstrap interval [0, +10]. The interval includes a tie, so this does
  not establish a Score win.
- Median maximum RSS: 42,688,512 baseline, 42,307,584 candidate. Paired median
  delta −380,928 bytes; 95% interval [−573,440, −327,680]. Candidate RSS was
  lower in all 11 pairs.
- Candidate Score was higher in 8 pairs, tied in 2, and lower in 1; the paired
  interval includes no negative values. Keep the change as a measured
  EarleyBoyer memory improvement for cumulative tuning, not as a Score or
  Stage B win.
- Candidate binary SHA-256:
  `ef44efdbab5c74177f23b610c72e65395a97f42627c5d9e088f9c7b757d8339f`.
- Fixture SHA-256:
  `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
- Node probe SHA-256:
  `540ca2d52fc247e8b9bbd83439b1b99ed084ab45a7dd72be015ad272ea54d10b`.

The full 11-pair report is [paired-11.json](paired-11.json), with individual
pairs in [paired-11.jsonl](paired-11.jsonl). The initial directional screen is
[screen-3.json](screen-3.json). `raw/` preserves stdout/stderr for all measured
runs. The runners and Node probe are included here.

## Method and host

The pinned V8-v7 fixture was materialized by
`work/task61-direct-number-screen/runner.py`. Baseline and candidate were
production-profile Quench binaries. Run order alternated baseline-first and
candidate-first. Intervals use 100,000 bootstrap resamples of paired median
deltas with fixed seeds. Host limits and tool versions are in [host.txt](host.txt).

The profile-guided approach is consistent with Casey, Gregg, Ertl and Nisbet,
[“Towards Superinstructions for Java Interpreters”](https://doi.org/10.1007/978-3-540-39920-9_23).
That work motivates testing common instruction pairs; the paired results here
determine whether this pair is retained.
