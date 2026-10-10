# Rejected EarleyBoyer local-to-binary fusion (2026-10-10)

The V2 Earley profile counted 22.7 million `LoadLocalPlain → Binary`
adjacencies. This experiment added left-local and right-local binary opcodes,
read the validated plain local directly, and ran the rewrite after local
specialization. The source change was reverted after the paired results.

The first three alternating production pairs looked positive: median Score
delta was +3 and candidate maximum RSS was lower in all three. Extending to
eleven pairs changed the result. Compared with the accepted `Move → Move`
binary, median Score fell from 397 to 391 (paired median delta −4; 95%
bootstrap interval −16 to +4). Median maximum RSS rose from 42,704,896 to
42,868,736 bytes (paired median delta +110,592; interval −327,680 to
+241,664). Score was lower in seven pairs and maximum RSS was lower in three.
The intervals include a tie, and neither metric established an improvement,
so this fusion was removed.

The baseline, candidate, and Node v24.19.0 agreed on the local arithmetic,
coercion, and field operand oracle. EarleyBoyer output also matched in all
eleven pairs. This was a valid negative performance result, not a semantic
failure.

The candidate's source patch, paired samples, runners, Node oracle, and raw
stdout/stderr are preserved here. Baseline SHA-256:
`07bd8382c10039f933c9462db39ad107168133e14acaaccbfa2618704475f6c6`.
Candidate SHA-256:
`67d710aea65bde6012b04fab90825e98d43e9cf631845b60e3516afd2d058717`.
Fixture SHA-256:
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

Research made dispatch fusion a reasonable hypothesis, not a presumption:
[Ertl et al., Vmgen](https://onlinelibrary.wiley.com/doi/10.1002/spe.434)
describe superinstructions among interpreter optimizations, while the
[ORBIT VM paper](https://research.ibm.com/publications/optimizing-r-vm-allocation-removal-and-path-length-reduction-via-interpreter-level-specialization)
reports gains from profile-driven specialization and instruction-path
reduction. This EarleyBoyer measurement did not support retaining this
particular fusion.
