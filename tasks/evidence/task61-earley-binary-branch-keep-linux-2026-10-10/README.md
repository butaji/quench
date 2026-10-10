# EarleyBoyer result-preserving binary branch fusion (2026-10-10)

The residual-scoped profile recorded 33,275,973 adjacent `Binary → JumpFalse`
executions. I prototyped a result-preserving opcode for sites where the binary
result remains live, in addition to the existing dead-result branch fusion.
The candidate passed the local Node v24.19.0 oracle, including falsey numeric,
string and BigInt results, coercion order, `instanceof`, and thrown-object
identity. The focused EarleyBoyer output also matched the accepted production
binary in all three alternating pairs.

The instrumented EarleyBoyer run dispatched the new opcode zero times. Its
profile recorded 9,149,904 existing `JumpBinaryFalse` dispatches. The feature
did not make the remaining hot pairs eligible for the result-preserving form.
The three production Score deltas were −1, +8, and −3 (median −1); RSS deltas
were −28,672, +69,632, and −225,280 bytes (median −28,672). These short,
directional measurements do not show a repeatable Score win or a meaningful
RSS change, so I removed the implementation.

Baseline binary SHA-256:
`ef44efdbab5c74177f23b610c72e65395a97f42627c5d9e088f9c7b757d8339f`.
Candidate binary SHA-256:
`1cb693a2e51824106f87208fea1a680a86a1e135f9a0634f926972dde7deaaa4`.
Fixture SHA-256: `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

See [screen-3.json](screen-3.json), the preserved [candidate patch](candidate.patch),
and [Node oracle](node-oracle.js). The three-pair report is a rejection screen,
not Stage B qualification.
