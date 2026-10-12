# EarleyBoyer Binary-to-Unary pair preflight

The aggregate-profile production build (`4a6b2b1d…`) executed 736,032,152
physical dispatches. The proposed general `SuperBinaryUnary` opcode executed
zero times. The retained strict `Binary → Unary → JumpFalse` prototype ran 928
times. I stopped before production scoring because this candidate has no
measured dispatch opportunity on EarleyBoyer.

The focused probe produced byte-for-byte identical output on Node v24.19.0,
the saved baseline (`e4653782…`), and the aggregate-profile candidate. This
checks the arithmetic, coercion order, BigInt error type, truthiness, and thrown
object identity; it does not alter the zero-frequency decision.

`candidate.patch` contains the removed prototype, `census.stderr` preserves
the aggregate census, `profile.stdout` preserves its fixture result, and
`oracle-results.json` records the three exact output rows and binary hashes.
`summary.json` records the fixture and profile binary hashes. No production
Score or maximum-RSS comparison was run.
