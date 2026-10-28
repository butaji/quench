# Splay ArrayRecord candidate correctness on M4

This records correctness and fixed-work evidence for the isolated
`v2-splay-array-record-next` candidate. It is not a trunk change or a
qualification result.

## Source and host

- Base commit: `f9e7123e96d36fd894a7d947fcba3864d1f9ca7f`
- Runtime diff SHA-256: `7cf60b799133b89397ce0a85c52130e1d5809dd0c75176a255c94bfeadabb22e`
- Host: Apple M4, macOS 26.5 (arm64), 10 CPU cores, 16 GiB RAM
- Test262 corpus: `f2d1435644797268dca1f7988cad5a4e89ccd8d2`
- WAST corpus: `67c8b9dc32c47d61c05a1681df349d01c85fa8ff`

The candidate worktree's WAST and Test262 submodule directories were empty.
The runs used the populated main-checkout submodules, whose commits match the
candidate's gitlinks exactly.

## Results

- `cargo test -p quench-runtime --quiet`: 555 unit tests and 6 integration
  tests passed.
- Shared Wasm suite: 67,124 passed, 0 failed; `inventory_complete=true`.
  Runner SHA-256: `11115dbfd7330714cb72f807afa3aa1ab411277ef67623149f99661d3256075e`.
  Report SHA-256: `ef5a29c47143b35fbf781f698308ef37eee31e412e6110582a226253355429ac`.
- Test262 Stage 31 Array: 3,081 passed, 0 failed. Report SHA-256:
  `477643f7fcd5e883c9bde04a12e9abd61dc11be55be911a8ad22581a2c2eb907`.
- Test262 Stages 60–63 Object, Promise, Proxy, Reflect: 4,604 passed,
  0 failed. Report SHA-256:
  `f2f1b1ca3d40e639614bd16b755bf5983a4aa16582da951e3a9c425582c95c77`.
- Test262 Stages 79–81 WeakMap, WeakRef, WeakSet: 255 passed, 0 failed.
  Report SHA-256:
  `5897997b7a7183bdeb3d1bdb7cbb52e73a5f70e91d8dc794b6ae39788ffe5437`.
  All Test262 runs used runner SHA-256
  `2f657766e3fb279d7ecfad24d955c4ba6cc4851226f1004cacebd1f484e97ee9`.
- Node-host probe: candidate `quench-node` and local Node v26.10.0
  `--jitless` exited successfully with identical output for sparse writes and
  deletion, slice copy-on-write, WeakMap array keys, Proxy `Array.isArray`,
  and JSON serialization. Candidate binary SHA-256:
  `9c2f9406f07ba8ff7d37e63a0b51ad53fafb9b1c2e49d9200f0e91b6f6b1e33c`.

## Fixed-work M4 screen

The pinned production candidate was compared with the current M4 trunk in an
11-round schema-5 all-eight run. Every fixture had 11/11 valid and clean pairs;
all fixture outputs matched. Host load before the final part of the run was
2.46 / 5.64 / 6.58. Candidate executable SHA-256:
`61373063e8d1ed13b1aafdc959e3b831b331db33fabb056a89129630eab8879a`.
Baseline executable SHA-256:
`8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`.
The baseline is the production binary for trunk `17077748a`; the candidate is
based on `f9e7123e96d36fd894a7d947fcba3864d1f9ca7f` plus the runtime diff above.

Splay marginal wall/run improved 7.546% (95% paired bootstrap interval
−8.731% to −6.249%), cycles improved 7.657% (−9.159% to −6.932%), and
instructions improved 5.412%. Work max RSS fell from 116,359,168 B to
85,295,104 B (−26.696%). This section records fixed-work results only; the
separate stock diagnostic is below.

Paired marginal wall deltas for the other fixtures were Crypto +0.519%
(95% interval −0.324% to +0.755%), DeltaBlue −4.804% (−4.908% to −4.471%),
EarleyBoyer −0.408% (−0.741% to −0.174%), NavierStokes −0.623% (−1.325% to
+0.893%), RayTrace −4.901% (−5.489% to −4.344%), RegExp +2.300% (+0.577% to
+3.072%), and Richards +1.027% (+0.558% to +1.314%). Work max-RSS medians
fell on all eight fixtures. A same-binary A/A on the exact trunk executable
also completed with 11/11 clean pairs per fixture. The wall A/A 95% intervals
were Crypto −1.018% to +0.681%, RegExp −1.720% to +1.247%, and Richards
−0.225% to +0.699%. Applying TOD-11's WP6 rule as a 2% allowance beyond each
A/A upper band, the corresponding candidate A/B upper bounds (+0.755%,
+3.072%, +1.314%) remain within those limits (+2.681%, +3.247%, +2.699%).
All eight A/B max-RSS intervals are below zero. The current M4 wall/RSS screen
therefore fits the WP6 other-fixture limits; no candidate stock Score was
measured in this run.

The candidate has point wall deltas above the 0.5% finding threshold on
Crypto (+0.519%), RegExp (+2.300%), and Richards (+1.027%). Crypto's interval
crosses zero; RegExp's and Richards' intervals are positive. These are filed
as measured findings, without attributing the changes to a specific path.

The pinned resumable guard classified the layout candidate `rejected` on its
cycle gate: Crypto +0.993% (95% interval +0.544% to +1.123%), RegExp +2.860%
(+1.150% to +3.792%), and Richards +1.088% (+0.442% to +1.609%). The guard
decision is kept distinct from the WP6 wall/RSS view. Decision SHA-256:
`fbf5c3306e289eaecfc54b97c889fa366101a76761bc6730dc6f5b193dad8e80`.
Raw report SHA-256:
`6a527a95f98823c34a7157b661b98209de60fe02aa0938f7dd603a8d187cfd96`.
Same-binary A/A report SHA-256:
`e47ff0d0f71f91c7d7b26f111186bf620335b75166f2e75d117a73c8d2288304`.
Derived wall/RSS summary SHA-256:
`1a4f6a1f8b2686eb3743782d2e3a665df32473c6fbb0c96c1a3f29c609c5a079`.

## Stock Splay diagnostic

After the fixed-work A/B and A/A controls, the candidate ran through the stock
V8-v7 harness for 11 valid rounds with equal output across engines. The report
records per-sample host snapshots; one-minute load stayed between 2.18 and
3.03, with no competing Deno workers observed. Engine versions were Node
v26.10.0 `--jitless`, Bun 1.4.2 with JIT disabled, and QuickJS 2026-06-04.

| Engine | Median Score | Median max RSS |
| --- | ---: | ---: |
| Quench candidate | 9,971 | 85,557,248 B |
| Node `--jitless` | 15,084 | 404,570,112 B |
| Bun no-JIT | 10,097 | 63,832,064 B |
| QuickJS | 8,744 | 151,060,480 B |

The score bar is Node and the RSS bar is Bun. Candidate distances are 1.513x
for Score and 1.340x for max RSS; it is not a win. The report has
`qualification_ready=false`. Its SHA-256 is
`a23ebf6efad5d7bb5339d6c574ebf8a9891a3ac860133d1b49b4c5bc9a5606a7`.

The official M4 trunk Splay row remains 1.664x Score distance and 1.810x max
RSS distance. The candidate is not merged and remains short of both bars;
there is no Linux result yet.
