# EarleyBoyer strict local-setter fusion

The merged Earley profile counted about 18.5 million adjacent
`LoadLocalPlain → SetThisFieldStrict` dispatches. This candidate fuses that
pair when the local is proven plain, has no numeric-update side effect, and
feeds the setter register. The opcode retains the original atom and cache
site; validation checks the local bounds, plain-local proof, and capture
status. The bytecode format version was raised to 82.

Node v24.19.0, the accepted production binary, and the candidate agreed on
primitive and object values, an inherited setter that throws, derived
constructor access before `super()`, and access after `super()`. EarleyBoyer
completed with equal observable output in all 32 alternating production
pairs.

The 32-pair Score result is neutral: both baseline and candidate median Score
were 398.5. The paired median Score delta was +1 point, with a 95% bootstrap
interval of −1.5 to +3; the candidate lost Score in 13 pairs. Maximum RSS
fell from a 42,770,432-byte median to 42,242,048 bytes. The paired median
delta was −499,712 bytes (95% interval −610,304 to −444,416), and candidate
RSS was lower in all 32 pairs. Keep this as a memory reduction for cumulative
Earley tuning; it is not a Score win or Task 61 qualification.

Baseline binary SHA-256: `07bd8382c10039f933c9462db39ad107168133e14acaaccbfa2618704475f6c6`.
Candidate binary SHA-256: `fc9011c45104a9389710e7f075f1fc41c36b40c7d073eb996c8cc6fbe2bdaf86`.
Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.
The candidate was built from source revision `674d255f8` plus the patch in
this directory. Raw paired data, runners, Node oracle, and captured process
outputs accompany this record.
