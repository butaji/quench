# TOD-132: shape-root tracing experiments

**Verdict: reject all three variants. No collector implementation or policy
change is retained.** The reproducible verification strategy is in
`docs/v2/gc-verification.md`. This experiment is tracked by
[TOD-132](https://linear.app/todoxapp/issue/TOD-132).

## Baseline and protocol

Linux x86_64, kernel 6.18.44, glibc 2.41, 4-CPU cgroup quota, Rust 1.99.0,
`production` profile, no guest JIT. Both sides were built at the same temporary
source path, `work/production-source`, starting from `cb077bb75`. The production
build logs and executable hashes are retained. The final documentation branch
also syncs later integration work; these results remain observations of the
pinned baseline, not measurements of subsequent integration commits.

Both sides used the existing link-order inputs. Candidate-specific retraining
on a disjoint corpus was not performed. The memory rejection therefore applies
to these builds; it does not establish that the algorithms regress RSS under
every retrained layout. Later integration refreshed those order files, without
changing or requalifying the archived prototypes.

The hypothesis and time/RSS decision rule were recorded in Linear before code
changes. Each prototype resolves shape transitions newest first, so deletion
suppresses historical roots and only the latest descriptor contributes accessor
roots. Symbols remain live through active property keys. The baseline continues
to reconstruct active attributes forwards.

The tokenizer diagnostic takes the pinned JetStream `generators/js-tokens.js`
and counts tokens in its own source ten times. Local Node v24.19.0 `--jitless`
produces `32990`; every measured Quench launch agrees. This is a fixed-work
diagnostic, not a JetStream score or a comparison against the project's Node
performance bar. Output formatting and first-use host initialization remain
part of the measured process peak.

## Results

| Variant | Patch | Tokenizer paired peak-RSS increase | Verdict |
| -- | -- | -- | -- |
| Inline masks for up to 64 slots; original large-shape fallback | `candidate.patch.gz` | 68 KiB; 95% paired interval [68, 68] KiB | Reject |
| Same algorithm, outlined helper | `outlined.patch.gz` | 64 KiB; interval [64, 64] KiB | Reject |
| Three-state scratch slots at every shape size | `slots.patch.gz` | 124 KiB; interval [124, 124] KiB | Reject |

Each result has 11 alternating paired rounds, warmups, successful exits and
matching stdout. The initial variant's median wall diagnostic falls from
1.2443 to 1.0619 seconds (−14.66%); the paired difference interval is
[−245.10, −134.13] ms. It nevertheless fails the predeclared memory rule. The
refinements' wall observations overlap compilation of their discarded test
builds and are **not used for time claims**; their RSS rejection suffices.
`tokenizer-pairs/`, `outlined-tokenizer-pairs/` and `slots-tokenizer-pairs/`
retain every launch, hashes and bootstrap details. Exact repeatable RSS values
are observed measurements, not uncertainty estimates covering other hosts or
build layouts.

The existing minimal `posix_spawn`/`wait4` helper independently confirmed the
initial RSS loss: 16,412 → 16,480 KiB. Relative-path screens of the outlined
and scratch variants gave 16,472 and 16,536 KiB respectively. Use the full
paired fixture results in the table: fixture path and launch context also
belong to a matched comparison. Sampled `/proc` memory reports are retained
only as diagnostics; their sampling missed brief peaks and cannot attribute
the process high-water precisely.

## Attribution and cross-workload guard

For the initial variant, production Callgrind instructions fall from
12,670,792,882 to 11,466,542,147 (−9.504%). Inclusive instructions in
`active_shape_attributes` fall from 1,342,990,799 to 153,327,589. The baseline
collector accounts for 86.36% of tokenizer instructions. These counters locate
a substantial cost, but do not establish an acceptable time/RSS tradeoff.
Compressed raw profiles, annotated summaries and `instruction-summary.json`
preserve the attribution.

The initial variant passes the all-eight instruction guard against fresh A/A
repeats. Largest increases are below 0.001%; RayTrace and RegExp decrease
1.25% and 1.17% per run. `guard-ab/` and `guard-aa/` retain materialized inputs
and raw counts (materialized scripts are gzip-compressed without changing bytes). A launch wrapper required successful process exit and the
`done` completion marker before accepting Callgrind's count. The script and
all launch logs are retained in `guard-launches/`.

The initial variant also completes 11 fixed-work pairs for every V8-v7 fixture:
all valid with matching output. Work peak-RSS medians are lower on all eight;
no positive RSS or wall-time interval excludes zero. The standard runner has
**zero clean timing rounds**, because hardware counters are unavailable.
`all-eight-diagnostic-summary.json` explicitly labels its raw-wall intervals
as unqualified Linux diagnostics. Do not treat Splay's raw wall delta as a
qualified mechanism gain. The M4 production gate and scoreboard are untouched.

## Correctness and completion checks

The initial variant passes `cargo check --workspace --all-targets` and all
604 runtime library tests under `iteration`, including independent forward
root-set comparison across the inline boundary and existing weak-reference,
ephemeron and finalization tests. `shape-lifecycle.js` matches Node and all
three prototypes under forced GC through accessor replacement, conversion to
data, deletion, re-addition and the 64-slot boundary.

The refinements' full runtime test builds were stopped after RSS rejection;
their partial build logs are retained. They are archived experiments, not
qualified implementation packages. No collector source changes survive them.

## Reproduction

Initialize the pinned benchmark submodules and build the baseline from
`cb077bb75` with `cargo build --locked --profile production -p quench-node`.
Preserve that executable. Decompress and apply one archived patch at the same source path,
build with the same settings and preserve the candidate separately. Validate
the patch before considering use; the scratch variant was not fully qualified.

From repository root:

```sh
bash evidence/tod132-shape-roots-linux-2026-10-14/tokenizer-pairs.sh \
  /absolute/baseline /absolute/candidate /new/output/directory
target/debug/quench-bench --all --fixed-work --runs 11 \
  --quench /absolute/baseline --quench-peer /absolute/candidate \
  --out /new/all-eight-pairs.json
bash evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh \
  /absolute/baseline /absolute/candidate /new/guard-output
```

The tokenizer wrapper reuses the existing repository's `wait4` and paired
percentile-bootstrap tool, with 100,000 resamples and its fixed seed. Profiling
used Valgrind 3.24.0. Run the forced-GC fixture with `--expose-gc` on Node and
Quench. Keep source, executable and fixture hashes when repeating the work.

The next independent experiments are complete payload/growth pressure
accounting (TOD-125) and direct-record young collection with a full-trace
verifier and mutation-barrier audit (TOD-124). Neither is enabled by this work.
