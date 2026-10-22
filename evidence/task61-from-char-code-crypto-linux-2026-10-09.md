# `String.fromCharCode` Crypto screen

This is a rejected focused Linux production experiment for Task 61.

The candidate added an exact one-argument numeric path to `String.fromCharCode`:
it reused the existing `ToUint16` conversion and directly allocated a string
containing one UTF-16 code unit. Other argument shapes still used the existing
implementation. This targeted the byte-to-character loop in the Crypto
fixture's `pkcs1unpad2` helper.

Node v24.19.0, the clean production baseline, and the candidate returned
identical oracle output for numeric conversion boundaries, zero and multiple
arguments, a custom call receiver, object conversion order, BigInt, and Symbol.
All eleven alternating baseline/candidate Crypto production pairs had matching
semantic output.

| Measure | Baseline median | Candidate median | Paired 95% interval for candidate − baseline |
| --- | ---: | ---: | ---: |
| Score | 153 | 154 | −6 to +3 points |
| Maximum RSS | 22,274,048 bytes | 22,568,960 bytes | −380,928 to +561,152 bytes |

Candidate Score was lower in six pairs; candidate RSS was lower in five. Neither
measure established an improvement. The implementation was removed.

The [paired JSON report](task61-from-char-code-crypto-linux-2026-10-09.json)
records binary and fixture hashes, host data, paired intervals, and samples.
The [raw pairs](task61-from-char-code-crypto-linux-2026-10-09.jsonl),
[reproduction runner](task61-from-char-code-crypto-linux-2026-10-09.py),
[candidate patch](task61-from-char-code-crypto-linux-2026-10-09.patch), and
[Node probe and output](task61-from-char-code-node-oracle-linux-2026-10-09.cjs)
are included alongside it.
