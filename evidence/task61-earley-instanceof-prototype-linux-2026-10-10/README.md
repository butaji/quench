# EarleyBoyer immediate `instanceof` prototype check (Linux, 2026-10-10)

The candidate returned `true` immediately when a plain `Cell::Object` had the
constructor's prototype as its direct prototype. Other object kinds and
prototype chains kept the existing traversal. This was motivated by the
EarleyBoyer opcode profile's 16.6 million `instanceof` operations.

Node v24.19.0 and the candidate binary produced the same result for direct and
derived instances, multi-hop and null-prototype objects, prototype mutation,
custom `@@hasInstance`, a proxy constructor, and a proxy instance. Output:
`INSTANCEOF_ORACLE:[true,true,true,true,true,true,true,true,false,false,false,true,1,1]`.
The normative algorithm is [OrdinaryHasInstance](https://tc39.es/ecma262/#sec-ordinaryhasinstance).

Eleven alternating production pairs on the pinned EarleyBoyer fixture had
identical observable benchmark output. Median Score moved from 210 to 211
(+1; paired-bootstrap 95% interval −3 to +7). Median maximum RSS moved from
50,585,600 to 50,216,960 bytes (−299,008; interval −1,753,088 to +245,760).
The candidate lost Score in four pairs, and RSS was lower in seven. Both
intervals include a tie, so the isolated fast path was rejected and reverted.

The baseline and candidate binary SHA-256 hashes are `e83200ec7666a02f8e4807fdb476eeb4dcef449ef469d15eefce1413f11369d5` and
`dd0f51e8e765e08d30accb128e5e3bb3cef857a67006e501f8cd3fdef03dd2b9`. The
fixture SHA-256 is `aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`; corpus revision is
`64e1860c736c1b899708f4cd646721bc71e53d8b`. The paired report and raw rows,
source patch, and Node oracle are in this directory.
