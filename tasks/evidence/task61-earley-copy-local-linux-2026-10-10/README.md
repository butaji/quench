# EarleyBoyer plain local copy fusion (2026-10-10)

The merged Earley profile counted 46.4 million adjacent `LoadLocalPlain → StoreLocalPlain` dispatches. I added a guarded `CopyLocalPlain` opcode that reads one validated plain local and writes another directly, removing the intermediate register operations. The rewrite rejects numeric-local updates, assignment-result stores, and self-copies; bytecode validation checks both local slots are plain and in bounds.

Node v24.19.0 matched the stable binary and candidate for copies of undefined, null, number, string, and object values; replacing the source after copying; assignment results; self-assignment; captured variables; and loop copies. EarleyBoyer benchmark output matched in all 32 alternating production pairs.

The 32-pair median Score moved from 367 to 376; the paired median delta was 3 points (95% paired-bootstrap interval -5 to 12). Candidate Score was higher in 17 pairs and lower in 15. The Score interval includes a tie, so this does not establish a Score win.

Median maximum RSS fell from 4.27704e+07 to 4.23076e+07 bytes (paired median delta -462848; 95% interval -512000 to -389120). Candidate RSS was lower in 31/32 pairs. Keep this as a measured memory improvement for cumulative EarleyBoyer tuning, not a Stage B qualification or a standalone joint Score/RSS win.

Baseline binary SHA-256: `f3940bcc9cff03f9bf3c6888864e3d96586d4991f4c56df4cc1e87d829e9fe00`. Candidate binary SHA-256: `6edef47edfd9bd995be5243ab4c83351db85a6abb2b0629c1dd82c00ead9eaa9`. Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`. The exact patch, oracle, runners, paired rows and raw stdout/stderr are preserved here.
