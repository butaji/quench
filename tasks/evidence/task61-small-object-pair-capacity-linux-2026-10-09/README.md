# Small property-vector capacity candidates

Both variants were rejected for Stage B. They reduced peak RSS on Splay, but
both reduced its Score with a paired interval excluding zero. The global
variant also regressed DeltaBlue Score and RSS in a three-pair control.

## Global two-slot minimum

The first variant changed the minimum `ValueArena` capacity from four slots to
two for all one- and two-property vectors. Its 11 alternating production
pairs on the pinned Linux Splay fixture produced equal output. Median Score
fell from 909 to 879 (−30 points, −3.30%; paired 95% interval −245 to +228),
so the Splay speed result alone was inconclusive. Median maximum RSS fell from
200,802,304 to 185,257,984 bytes (−15,544,320; paired interval −15,794,176 to
−15,265,792), lower in all 11 pairs.

The one-pair all-eight screen had mixed directions. Score increased on five
fixtures and fell on DeltaBlue, EarleyBoyer, and Splay. RSS fell on Crypto,
EarleyBoyer, RayTrace, Richards, and Splay, and rose slightly on DeltaBlue,
NavierStokes, and RegExp. A three-pair DeltaBlue control confirmed a joint
regression: median Score fell 3.8 points and median maximum RSS rose 921,600
bytes, with RSS higher in all three pairs. A three-pair EarleyBoyer control
reduced median Score 7 points and RSS 2,301,952 bytes. These controls reject
the global minimum.

## Direct-return two-slot vectors

The refinement compacted only two-property objects whose bytecode returns
the result directly from the frame. Normal object pairs retained four slots;
a compact vector grew through the normal path if a caller later added a
property. This uses the return-result bytecode flag and adds no benchmark-name
or fixture check.

Eleven alternating production pairs on Splay matched output. Median Score
fell from 871 to 812 (−59 points, −6.77%; paired 95% interval −221 to −1),
with the candidate scoring lower in 8/11 pairs. Median maximum RSS fell from
200,777,728 to 185,253,888 bytes (−15,523,840; paired interval −15,654,912 to
−15,376,384), lower in all 11 pairs. The reliable memory gain does not offset
the reliable Score loss, so this refinement was removed.

Node v24.19.0, the baseline binary, and the candidate binary produced
byte-identical JSON for 5,000 returned two-property objects, later property
addition/deletion, descriptors, enumeration, and duplicate keys. The focused
`compact_two_value_vectors_grow_normally` unit test passed. The full runtime
library suite reported 470 passed and 52 failed on the candidate. A
representative `WideInstruction::local_slot` validation failure reproduced
on clean `v2-cloud` HEAD, so that failure predates this candidate; the full
failure set was not established as pre-existing.

`global-minimum-pairs.json` and its JSONL preserve the 11 raw Splay pairs;
`global-all8-screen.json` and `global-*-three-pairs*` preserve the screen and
controls. `return-site-pairs.json` and its JSONL preserve the return-only
candidate's 11 pairs. Candidate source, both runners, and the Node oracle are
included. Binary hashes, fixture hashes, corpus revision, host limits, sample
order, Score, and Linux `wait4` maximum RSS are recorded in the raw reports.

These are Quench-only optimization experiments, not the Task 61 all-engine
qualification. Quench still does not lead Score or maximum RSS across all
eight V8-v7 fixtures; Task 61 remains open.
