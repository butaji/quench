# Splay record-space bitmap allocator screen (M4)

## Refreshed gate on the current base

The first screen in this README used base `761903894` and is historical. The
current candidate was rebuilt after syncing to `f76795545b4f2f8cc7cf423da49e758215525b9f`:

- Parent binary: `c6e60e98592418ca595b1bb15aff2005e5388a321a757fbafae9723155d7e1ca`
- Candidate binary: `d71b870433be8173470951e76872e2cddb71fd0723cbb6c2d7695ffbaf345ebe`
- Runner: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`
- A/A-adjusted WP6 summary: `all-eight-wp6-aa-adjusted-summary.json`, SHA-256
  `3f7f7bc3a40a699b6861df5d5a7dc033ef189e8ffff877cc0dc5d4dfb32263be`

The schema-5 A/A and A/B screens are output-equal, with at least 11 clean pairs
for every fixture. The A/A-adjusted WP6 screen passes: Splay work max RSS falls
7.50% (127,254,528 → 117,719,040 B; 95% interval −7.524% to −7.485%). Splay
fixed-work wall is −0.500% [−1.259%, +1.959%] and cycles −0.252%
[−1.270%, +2.510%], so this change makes no speed claim. Crypto's RSS excess
over its A/A 95% upper band is +0.524 percentage points; DeltaBlue's wall excess
is +1.119 points. Both remain within TOD-11's WP6 limits and their required
findings are filed on TOD-16 and TOD-13. The generic guard's `rejected` label
uses an unadjusted 0.5% RSS limit and is not the WP6 decision.

All three `heap::records::tests` pass. The full debug runtime suite has the same
318 failing test names on parent and candidate (260 vs 261 passing; the
candidate adds the passing bitmap-order test); the reproduced failure is the
existing direct-record full-mark verifier assertion at `heap.rs:1630`. The
release-profile runtime test build was interrupted after almost ten minutes in
rustc thin-LTO compilation and did not complete; this is not a pass. The full
WAST suite passes, 67,124/67,124. Two earlier stock attempts remain excluded
for host contention; a later clean stock diagnostic is recorded below.

## Refreshed stock diagnostic

Apple M4 arm64 macOS, schema-5 stock harness, 11 valid rounds per engine,
output-equal. Report `splay-stock-11.json` (SHA-256
`9f43c2784b7a9a194c6f2978c38e789e808bcb29d36acb89fbcf482b28d546bb`) has 88
host snapshots; maximum one-minute load was 3.99 across 10 logical CPUs, and no
Deno, Cargo, rustc or esbuild worker appeared in the sampled top CPU consumers.

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench candidate | 9,124 | 117,735,424 B |
| Node `--jitless` | 14,766 | 406,323,200 B |
| Bun no-JIT | 9,901 | 65,601,536 B |
| QuickJS | 8,557 | 151,044,096 B |

The best same-campaign Score bar is Node and the lowest RSS bar is Bun. The
candidate is still 1.618× behind the Score bar and 1.795× above the RSS bar.
It remains isolated, so these are candidate distances only; the official trunk
scoreboard is unchanged. Derived values and all input hashes are in
`stock-candidate-summary.json` (SHA-256
`d95cb46dd2f2ba757016749094d6244eab572c3208e33484076baa73ba45cd13`).

This is a focused Splay diagnostic for replacing the `RecordSpace<T>::free`
ID vector with a cursor that finds reusable IDs from the occupied bitmap. It
preserves the existing descending ID reuse order. The parent is `761903894`
with the allocator patch absent; the candidate adds only `allocator.patch` to
the runtime. Both binaries also include the same temporary macOS compile
compatibility patch in `m4-build-compat.patch`.

## Result

Apple M4 arm64, macOS. Both builds used the production profile, thin LTO,
single codegen unit, and 64-byte function alignment. The merged base currently
fails its warning-denied M4 build on unrelated Node-module diagnostics, so the
matched builds used `--cap-lints allow` after retaining the configured
`-D warnings`; this changes lint enforcement only. The temporary macOS signal
table matches local Node's `os.constants.signals`, and the non-Linux unused
argument fixes are identical in both builds. These are build-only controls,
not part of the allocator candidate.

The pinned executable hashes are:

- Parent: `4d3ce990fbb675dfea95e81d65f644dffc217804287564ab079aa5f4753c08c4`
- Candidate: `b7484bbf994a6fd7eae2d3dcbc634edbe11bbf03e8b72f663e99b0e63d7015c2`
- Measurement runner: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`

The runner used 300 Splay `run()` calls per work sample and an 11-round
setup-subtracted pair. Same-binary A/A was valid, output-equal, and clean in
11/11 rounds. The candidate A/B was valid and output-equal in 11/11 rounds,
with 10/11 clean pairs. On those 10 clean pairs, paired marginal changes were:

| Metric | Median | 95% paired interval |
| -- | --: | --: |
| Wall per `run()` | −0.680% | [−1.211%, +1.179%] |
| Cycles per `run()` | −0.017% | [−1.054%, +0.942%] |
| Instructions per `run()` | +0.003% | [−0.012%, +0.022%] |

Work max RSS fell from 127,254,528 B to 117,719,040 B (−9,535,488 B,
−7.50%; paired 95% interval [−7.524%, −7.485%]). The focused Splay result is
an RSS win with speed neutral within its paired interval.

The all-eight A/A and A/B outputs match. The resumable guard was run with
TOD-11's 11-clean-pair policy (the temporary one-line override is preserved in
`guard-11-clean.patch`). The guard passed: each fixture has at least 11 clean
pairs, with Splay at 17 clean / 18 valid after a seven-round Splay-only top-up.
No fixture has a detected cycle regression or an instruction/RSS budget
failure. Splay marginal cycles improved 0.693% (95% paired interval
[−1.571%, −0.473%]), instructions were +0.002%, and work max RSS fell 7.482%.
The paired marginal wall median across its 17 clean pairs is −1.236%.
RayTrace also improved 0.671% in cycles; the other six fixtures' cycle
intervals include zero. The guard decision is
`all-eight-guard-checkpoint.decision.json` (SHA-256
`8b0094df8d0dd653b6d2014c8c67ec292fd5396e831cd9cfd0bbe392e4231cf5`).

Two stock-harness runs completed 11/11 and matched outputs, but both are
excluded for contention. In the first, Deno/Node/Bun CPU bursts appeared from
rounds 4–10; it is preserved as `splay-stock-contended-11.json` (SHA-256
`33163819eac158b9f1b36a9a7a440c6bf64ca538a9e5b1f9a7a18443c45b1c0b`). In the
second, Deno was already visible in round 0 and remained active through round
10; it used the clean pinned benchmark corpus and is preserved as
`splay-stock-contended-clean-corpus-11.json` (SHA-256
`0e4bbbfaf66fd2b93b93d9849f2fcf6afe38b935a33dc8829594e69d7025eaa8`). A clean
stock retake is still required, so official task-61 Score/RSS distances do not
change.

Raw report hashes:

- Same-binary all-eight A/A: `all-eight-aa-11.json`, SHA-256 `c1760c3dd7c655076c385b937e6463ec84de4f10d699835eef603d85906fe0cf`
- All-eight A/B: `all-eight-ab-11.json`, SHA-256 `75b3e5310d602c2c05146fee61264df12c2b492833d98e5feaa718f736f4c30c`
- Splay-only A/B top-up: `all-eight-guard-checkpoint-topups/splay.js-batch-1.json`, SHA-256 `21dbfa5191fb6d2c41704910d964dc45e429222d37dd110667f0173d9520c723`

## Reproduction

From the main checkout, using the two pinned executables above:

```sh
target/candidate-b/production/quench-bench \
  "$PWD/quench-bench/js-engine-benchmark/v8-v7/splay.js" \
  --fixed-work --runs 11 \
  --quench /private/tmp/quench-task61-splay-free-bitmap-m4/target/pinned/4d3ce990fbb675dfea95e81d65f644dffc217804287564ab079aa5f4753c08c4/quench-node \
  --quench-peer /private/tmp/quench-task61-splay-free-bitmap-m4/target/pinned/b7484bbf994a6fd7eae2d3dcbc634edbe11bbf03e8b72f663e99b0e63d7015c2/quench-node \
  --out /tmp/splay-record-free-bitmap-ab.json
```

Raw focused Splay A/A and A/B reports are `splay-aa-11.json` and
`splay-ab-11.json`; `summary.json` records paired-bootstrap inputs, build
flags, and report hashes.
The A/B's invalid-for-cycles contention sample was excluded from wall, cycle,
and instruction paired statistics; RSS uses all valid rounds. The interval
uses the fixed-work guard's 20,000-resample xorshift64 bootstrap procedure.
Because this focused report selected only `splay.js`, its suite-wide
`fixture_set_matches` field is false by design; the separate all-eight reports
use `--all` and the pinned eight-fixture corpus.

## Base refresh after lane sync

The initial allocator gate above used base `761903894`. Before retaining the
candidate, the branch was fast-forwarded to `origin/v2-merged` at
`f76795545b4f2f8cc7cf423da49e758215525b9f`. The four intervening commits
modify x86-64 Wasm lowering/link order; the JavaScript runtime source is
unchanged. Nevertheless, rebuilding the M4 production binaries changed both
hashes, so the earlier fixed-work and stock reports are stale for the refreshed
candidate. The refreshed parent and candidate are pinned as:

- Parent: `c6e60e98592418ca595b1bb15aff2005e5388a321a757fbafae9723155d7e1ca`
- Candidate: `d71b870433be8173470951e76872e2cddb71fd0723cbb6c2d7695ffbaf345ebe`

The refreshed 11-round Splay A/A, Splay A/B and all-eight regression guard
complete with the WP6 pass above. The stock diagnostic is complete, but neither
Splay bar is won and the candidate is not integrated.

## Matched d437 bitmap-cursor result

The current matched parent/candidate pair is based on parent
`d437123d995a2194722f179d901a54e227c546e1` and candidate
`8fc956af62ca502def4a0d246ee42d62b4db9c3b`. The earlier d437 screens launched
from the main checkout at `597dd55d4` are invalid because the runner recorded
the wrong source root; those raw reports remain with `wrong-source-root` in
their names for audit only. The corrected A/A and A/B runs started from their
respective clean worktrees, both on corpus
`64e1860c736c1b899708f4cd646721bc71e53d8b`.

The schema-5 fixed-work A/A and A/B reports plus top-ups pass the resumable
all-eight WP6 gate. At least 11 clean pairs are available per fixture, outputs
match, and the A/A-adjusted thresholds pass. Splay marginal wall is +0.7975%
with 95% interval [−0.3555%, +1.1586%], so this is neutral for speed. Work max
RSS moves from 122,634,240 B to 117,407,744 B, a paired median reduction of
4.324% [−4.4204%, −4.1951%]. The exact paired/bootstrap calculations are in
`all-eight-wp6-summary-d437.json`.

The current-source stock Splay retake has 11 valid rounds per engine and
matching outputs. Its sampled one-minute host load ranged from 3.05 to 4.89
across 10 logical CPUs, with no Deno, Cargo, rustc or esbuild worker in the
recorded top consumers. Median scores are Quench 9,176, Node `--jitless`
14,782, Bun no-JIT 9,926, QuickJS 8,630. Median max RSS is Quench 117,440,512
B, Node 408,387,584 B, Bun 65,880,064 B, QuickJS 151,044,096 B. The best
same-campaign score bar is Node and the lowest RSS bar is Bun: candidate
distances are 1.61× for Score and 1.78× for RSS. The candidate remains far from
the Splay win condition.

The stock report is complete and the Splay fixture is valid/output-equal, but
the runner's `qualification_ready` is false because its corpus check expects
the full eight-fixture set while this focused invocation intentionally selected
only Splay. Treat these as a valid Splay diagnostic, not an all-suite
qualification report. The official trunk scoreboard remains unchanged because
this candidate is isolated.

Correct-source evidence hashes:

- Parent A/A: `all-eight-aa-d437.json`, SHA-256
  `d39f259ab5b05ae7878f86daa9ec858123209c866202ea9923d7991e0f4a7b58`
- Parent Crypto A/A top-up: `all-eight-aa-crypto-topup-d437.json`, SHA-256
  `a8fb0d667c6f4620601a2e244ef045d7b590a0adfaa1afaa4f12b0878a3d2d1e`
- Candidate A/B: `all-eight-ab-d437.json`, SHA-256
  `ae07ab699dd41e690690252538dd08e1d3f991cb985e0f4ecb145f666344e47b`
- Candidate RayTrace A/B top-up: `all-eight-ab-raytrace-topup-d437.json`, SHA-256
  `e93c68f8bc93062c464a6172bd2c2bb1a8bfc650475a4b28fd61bd8bc9cd80e9`
- WP6 summary: `all-eight-wp6-summary-d437.json`, SHA-256
  `496c8ee167b9f55d8053ea24f0b0dab94e4cfd7f9b35333f571be5f79dec2eee`
- Stock Splay report: `splay-stock-d437.json`, SHA-256
  `8a2a03d2650636df5d621f5d1b482a19ee6d13d94a5e892c993a46d7aa84f913`
- Full WAST report: `wasm-d437.json`, SHA-256
  `d3f4fcc8dfe012ba22059d25e88cc9bd3aabfdb32c47ea411c1be5804472fce0`

The two invalid wrong-source-root reports are preserved separately. Pinned
candidate binary SHA-256 is
`3c9b0c25ecdd939762d86b2e39275e02cd4aab5f20adc97c811b9048a73968bf`, parent
binary SHA-256 is
`4eafb40ab6ab97569f48fe733f0b4b297bc2e37d0333bdf8153c61255c85fe4d`, and the
stock runner SHA-256 is
`bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.

## Splay record-space residency attribution

Using the same pinned candidate binary and corpus, `capture_vmmap_d437.py`
stopped the process after `Setup()` and after 300 `run()` calls. The host was
M4 arm64 macOS with 10 logical CPUs; load immediately before capture was
7.16 / 9.15 / 8.95. These are process snapshots, not stock-harness RSS or
timing results.

| Stage | `ps` RSS | Default malloc-zone resident | `MALLOC_LARGE` resident | `MALLOC_SMALL` resident |
| -- | --: | --: | --: | --: |
| Setup, K=0 | 76,896 KiB | 62.1 MiB | 25.6 MiB | 35.8 MiB |
| After 300 runs | 114,368 KiB | 95.8 MiB | 53.6 MiB | 39.5 MiB |
| Change | +37,472 KiB | +33.7 MiB | +28.0 MiB | +3.7 MiB |

The two record spaces explain the large mappings: ObjectRecordSpace reserves
512 MiB (`16,777,216 × 32 B`) and ArrayRecordSpace reserves 640 MiB
(`16,777,216 × 40 B`). At K=300, `vmmap` shows matching 128 MiB mapped portions
with 31.6 MiB and 19.0 MiB resident, respectively; their setup residents are
15.8 MiB and 9.8 MiB. This is a strong address/size match to the record spaces,
not an allocator-owner label emitted by `vmmap`. The process RSS increase is
concentrated in those record mappings; `MALLOC_SMALL` rises by only 3.7 MiB.
This points the next Splay memory step at the per-record layout. It does not
claim that reducing record width will save an equal amount in stock RSS.

The work snapshot is taken after the last run and can miss a transient peak.
Raw map, summary, process and source files are in `vmmap-d437/`; metadata
records both source hashes and the pinned executable SHA.
