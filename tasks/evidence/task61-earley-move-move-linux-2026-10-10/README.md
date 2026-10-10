# EarleyBoyer Move-to-Move fusion (2026-10-10)

The merged profile counted 23.1 million adjacent `Move → Move` dispatches. I added the pair to the existing producer-move rewrite. It reuses the first move with the second destination; the normal liveness guard keeps the pair unless the first destination is dead. No opcode or runtime behavior changed.

Node v24.19.0 matched baseline and candidate for chains carrying undefined, null, booleans, numbers, strings, and objects through branch and replacement cases. EarleyBoyer output matched in all 50 alternating production pairs.

Compared with the committed `CopyLocalPlain` baseline, median Score rose from 358 to 368; paired median delta was +11 points (95% bootstrap interval +4 to +14). Score was lower in 16/50 pairs.

Median maximum RSS fell from 42,770,432 to 42,307,584 bytes (paired median delta -417,792; 95% interval -524,288 to -356,352). Candidate RSS was lower in all 50 pairs. This is a joint Quench-only EarleyBoyer improvement, not an all-engine Stage B qualification.

Baseline binary SHA-256: `6edef47edfd9bd995be5243ab4c83351db85a6abb2b0629c1dd82c00ead9eaa9`. Candidate binary SHA-256: `07bd8382c10039f933c9462db39ad107168133e14acaaccbfa2618704475f6c6`. Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`. The patch, Node oracle, runners, paired report, rows, and raw stdout/stderr are preserved here.
