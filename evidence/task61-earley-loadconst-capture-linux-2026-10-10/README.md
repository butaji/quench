# Rejected EarleyBoyer `LoadConst → LoadCapture` fusion

The candidate fused adjacent constant and capture loads into one dispatch,
preserving both register writes and their order. Node v24.19.0, the baseline,
and both candidate builds produced identical results for nested and mutated
captures, null, negative zero, numbers, and strings. EarleyBoyer output matched
in every production pair.

The scoped baseline profile counted 17,492,201 adjacent
`LoadConst → LoadCapture` executions. The rewrite protects jump, exception,
parameter, and binding-site entries; its instrumented census recorded
1,516,923 fused executions.

The decision uses production binaries built with the same workspace path and
toolchain. Baseline source is commit `5a3397e86`, SHA-256
`e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`. The
original candidate SHA-256 is
`2fdbe967d451fbad57a44fee77fbd1bbdb86ceade7ecae5762e3e6cca672ee9a`; the
rebuild with shared Nop padding SHA-256 is
`4a94470869fe5aa592c6886092326aafe5f912dcb8a86da8f8094f26fc73b751`. The
materialized fixture SHA-256 is
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
Measured binaries are kept in the ignored
`work/task61-v2-merge-earley/loadconst-capture-binaries/` directory; their
hashes and source patches are retained here.

The original candidate was measured in four exact-baseline blocks totaling 44
pairs. Their combined median Score delta was +3.5, with interval [0, +5]; the
median RSS delta was +2,048 bytes, with interval [−61,440, +65,536]. A final
22-pair block instead had median Score delta −4.5 (interval [−8, −2]) and
median RSS delta +83,968 bytes (interval [+32,768, +126,976]).

Across all 66 pairs for the same binary hashes, median Score delta was zero
(95% bootstrap interval [−2, +3]); Score rose in 29 pairs, tied in five, and
fell in 32. Median maximum RSS increased by 36,864 bytes (interval [−8,192,
+83,968]); RSS was higher in 38 pairs, lower in 27, and tied once. The combined
report therefore does not establish a Score or RSS win. Remove the fusion.

A rebuilt padding form, which used one shared Nop value in the superinstruction
table, was also run for eleven pairs against the same baseline. Its median
Score delta was zero (95% interval [−6, +7]); median RSS delta was −73,728
bytes (interval [−311,296, +24,576]). That form also did not establish a win.

An initial eleven-pair run against a detached-worktree baseline showed a
positive Score interval, but its baseline hash differed from the reproducible
same-path build. I excluded that run and rebuilt against the exact
`5a3397e86` binary before reaching this decision.

This Linux, one-fixture experiment does not qualify Stage B or establish the
all-eight, all-engine Apple M4 gate. `combine.py` reproduces the 66-pair
summary in `paired-combined-66.json` and `pairs-combined-66.jsonl`. Individual
exact-baseline reports are `paired-screen-root.json`, `paired-confirm.json`,
`paired-confirm-original.json`, `paired-confirm-round3.json`,
`paired-confirm-round4.json`, and `paired-confirm-round5.json`; the alternate
padding report is `paired-confirm-exact.json`. The detached-worktree screen is
retained separately. The runner saves production stdout/stderr under
`all8-samples/`; Node output, profile census, source patches, and binary hashes
are included for audit.
