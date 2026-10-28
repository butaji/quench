# Rejected EarleyBoyer local field-get fusion (2026-10-10)

This candidate fused `LoadLocalPlain -> GetField` when the field base was the loaded local and the field lookup was an atom. `GetFieldLocal` retained the existing cached property-get path. The compiler also required valid narrow encoding and passed the usual liveness rewrite check.

The focused Node v24.19.0 oracle matched the baseline and candidate for own and inherited properties, a getter receiver, method receiver, proxy get count, a caught getter exception, and null-base exception type. The full EarleyBoyer benchmark outputs matched in all three alternating production pairs.

Median paired Score delta was -12 points and median paired maximum-RSS delta was -69632 bytes. Score deltas were [0.0, -24.0, -12.0]; RSS deltas were [-12288, -69632, -446464]. Candidate RSS was lower in all three pairs, but Score lost in two and tied in one, so this is not a joint Score/RSS win and the candidate is removed. Three pairs are a directional screen, not qualification evidence.

The candidate binary SHA-256 is `b338b1371167653151ef5bcf1f3af83123c95dbddb348e087feab9d0ca2dce3d`; the stable baseline binary SHA-256 is `f3940bcc9cff03f9bf3c6888864e3d96586d4991f4c56df4cc1e87d829e9fe00`. The fixture SHA-256 is `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`. The patch, runner, oracle, pair report, and raw stdout/stderr are preserved alongside this note.
