# npm module graph qualification, 2026-10-15

**The requested all-function parity and subsequent no-JIT package wins remain
open.** Published functional milestone: e17120a3c on v2-merged and v2-npm. Latest
completed 300-package binary snapshot: 606439b9e against official stable Node
26.12.0, with 268 API passes, 13 declaration passes, 18 Quench failures and one
Node platform failure. Descriptor discovery has 151 unqualified matches,
140 differences and nine oracle failures. Only 339 of 70,236 observed callable
paths (aliases included) have matching retained contracts; 69,897 are uncovered.
These reports include the retry repair and predate the later 1d7439285 integration refresh.

## Retained repairs

0f2ee536 links ESM graphs before executing bodies: environments, hoisted functions,
canonical namespace shells, live aliases and typed JSON reexports derive from
compiled link plans and resolved dependency identities. bb128213a resumes valid
modules left unlinked by a failed parent. Its optimized runtime suite passes
617 tests. The earlier 0f2ee536 suite exposed that retry failure; its failure log
remains retained rather than replaced by the later pass.

7b5fcb815 matches Node's CJS lexical export prefix, including semver's missing
namespace.re alias, member-valued properties, methods, spreads and required
reexports. Its expanded Node oracle fixture passes. 1af9dce0f tracks regular
files through Linux directory watches, routes directory filenames to entries,
forwards ref/unref to actual watchers and fixes missing-root error policy. Eight
recursive-watch fixtures pass, including mutation, ignore, links, iterator and
lifecycle checks. Its TLS fixture now keeps the client open until directed server
destruction: racing shutdown on both peers made Node itself alternate between EOF
and ECONNRESET. Ten directed pairs match, and the revised Rust fixture passes.

8528d3f6b matches the changed Node 26.12 loader lifecycle. Every pre-evaluation
failure can retry; evaluation errors remain cached. Each queued import owns its
own instantiation attempt, while the module record still shares successful
execution. Missing-export errors retain the requested specifier and name. The
expanded fixture checks fresh concurrent errors, parse retries, cached evaluation
failure and later valid imports. Shared runtime coverage exercises both compiler
modes. All 618 runtime tests pass in the default unoptimized correctness profile.
The older source-phase test's cached-error assertion was separately checked and
updated against Node 26.12; the previous failing run is retained.

Other changes in 0f2ee536 project module stack frames to file URLs, restore Buffer's
forward UTF-16 string offset alignment, match Node 26 queued abort-listener event
and cancellation behavior, and remove obsolete crypto/TLS engine warnings.
Source-kind checks inspect package JSON directly instead of allocating resolver
caches, and canonical exact files precede general resolution. A date-fns import
fell from approximately 9 GiB to 23 MiB RSS in an isolated diagnostic; this is not
a package benchmark victory. Both npm and integration package-scope boundary
rules survive the merge.

## Integration and qualification

The completed 606439b9e snapshot includes integration 8d592cfb3, its stable Node
HTTP status cache fix, web repairs and shared Promise changes. The Node source
submodule is pinned to official 26.12.0 commit 26e72d85a6. Workspace/all-target
checks, all 618 default-profile runtime tests and all 618 iteration-profile runtime
tests pass. Fresh complete package results are 268 API/13 declaration passes,
18 Quench failures and one Node platform failure; surface and callable counts
remain 151/140/9 and 339/70,236 respectively. The additional pass is undici.

The independent broader Node fixture run is 235/244: nine failures remain and are
retained verbatim. Three cases trigger bundled Undici while a user Buffer binding
is in TDZ; other cases concern watcher hooks, Stats identity, close-event flags
and child-process behavior. The watcher hook failure reproduces identically on
the clean 8d592cfb3 integration binary and the npm candidate: both have no hooks,
while Node observes root/link/sub and ref/unref forwarding. This is an integration
finding, not a repaired npm graph case. TOD-134 owns the web/builtin initialization
finding. No whole-Node-suite or total npm parity claim is made.

The lane subsequently merges integration 1d7439285 (lane source 23d4b4bbb), including
canonical stream emitter/duplex-pair and shared dispatch changes. Workspace checks
and all 618 default-profile runtime tests pass there. The 606439b9e binary reports
remain explicitly historical for those newer integration changes.

Earlier broad fixture runs are historical: 0f2ee536 passed 237/239; 8c9bedaf8
passed 240/241 against Node 26.11.1. The sole latter failure was the HTTP status
getter difference subsequently repaired by the Node lane. Against newly installed
26.12.0, the older 8c9bedaf8 fixture binary passed 238/241: the source-link identity
expectation changed, the HTTP getter differed, and a 500 ms child timeout test
failed during heavy concurrent compilation. That timeout fixture passes isolated;
its earlier failure remains visible. New combined fixtures must replace these
historical observations before a current whole-suite claim.

The matched 5f1ccdc91-to-8c9bedaf8 instruction screen uses opt-level 1 and disables
Cargo incremental builds in both binaries. Maximum per-run regression is +0.40%.
It includes intervening integration changes, so does not isolate npm attribution.
The earlier screen used mismatched incremental settings and remains explicitly
diagnostic. The completed 8d592cfb3-to-606439b9e screen has maximum per-run regression
+0.31%; all 32 independent completion checks pass. Candidate and baseline use the
same iteration settings with incremental disabled. A shared Cargo target first
supplied an incompatible baseline Host trait to the candidate CLI build; that
build was rejected and its error retained. Final candidate and fixture binaries
were rebuilt in an isolated target directory. verify-guard.py requires all scripts
to exit zero with exact completion output and empty stderr, independently of
Callgrind's instruction totals. No package performance win is implied.

Node 26.12.0 archive SHA-256 matches its official release manifest; verification
metadata is retained. Historical Node 26.11.1 snapshots keep their original engine
hashes and are not substituted for 26.12 observations. Report source metadata may
precede the commit of source-equivalent changes; executable and fixture hashes
identify the observed snapshot. snapshots.json distinguishes the binaries.

The user requested performance measurements and Quench wins across these packages
with JIT disabled after parity. That phase has not started. Quench JS uses its
shared interpreter. Node 26.12 does not expose WebAssembly under --jitless
--wasm-jitless; this is an oracle limitation, never a performance win. No whole-
package, all-input or production speed claim follows from this evidence.

Linear TOD-127 owns the overall goal; TOD-128/TOD-129 retain module and capability
qualification. Remaining host blockers and callable obligations stay open.

The next util.parseArgs capability has an unchanged upstream 117-test oracle
baseline. It passes 117/117 from a standalone Node source checkout. When nested
under this repository, a CommonJS child fixture inherits the parent's type:module
and Node itself fails one case. Both observations and exact fixture hashes are
retained; this setup failure is not a Quench API result. Quench still lacks the API.
