# Top-300 npm parity evidence, 2026-10-14

Qualification is incomplete. This directory retains real published-package checks
against Node v26.11.1, failed observations and a per-callable uncovered ledger.
The goal and capability findings remain open on
[TOD-127](https://linear.app/todoxapp/issue/TOD-127), TOD-128, TOD-129 and TOD-130.

The 300 version/integrity pins and reproducible commands are in
[`tests/npm-top300`](../../tests/npm-top300/README.md). Popularity is weekly npm
downloads among a retained 4,665-candidate snapshot, not a complete registry census.
The install uses TypeScript 6 for supported ESLint peer context and an explicit
TypeScript 7 alias for the selected package. Native musl/glibc artifacts and npm
peer conflicts remain visible; neither forced installation nor a host CLI launch
qualifies native addon APIs.

## Retained snapshots

Files ending `490a0461f` retain the earlier integration snapshot. Files ending
`0be8440e0` retain the candidate after merging RSS integration `d8583fde3`:
247 passing API scenarios, 13 passing declaration-consumer checks, 39 Quench
failures, one Node/platform oracle failure. Surface discovery has 147 descriptor
matches (explicitly unqualified), 144 differences and nine oracle failures.
The observed callable inventory contains 70,236 paths, including aliases;
271 paths have passing behavior contracts and 69,965 remain uncovered.
`summary-0be8440e0.json` contains these counts. Gzip files decompress to full JSON,
including both engines' outputs, status, deadlines and executable/source hashes.

A descriptor match never proves function behavior. Successful scenario output
without callable contract tags supplies zero function coverage. Dynamic/returned
APIs, complete upstream suites, declaration contracts and CLI behavior remain
separate obligations. `npm-coverage` therefore returns nonzero and records
`qualified: false`; it cannot turn smoke checks into all-functions parity.

The semver contract executes 2,515 assertions and checks all 101 discovered
callable paths. Its 49 production JS files match the published package. The full
upstream TAP suite remains a failure on Quench: `upstream-semver.json` records
commands, executables, deadlines and status. Older text output alone is not a pass.

## Final scenario run

`scenarios.json.gz` is the complete latest 300-package run at 8a1cc805c, using
the executable also rebuilt unchanged at final source 83242b53a: 248 API scenarios
pass, 13 declaration-consumer checks pass, 38 Quench failures, one Node/platform
oracle failure. The upstream socket/stream changes make the ora scenario pass.
This is representative API coverage, not all-functions qualification.
`surface.json.gz` and `coverage.json.gz` retain the corresponding completed
300-package callable run. See `summary.json` for the final counts. 271 of 70236 observed
callable paths have passing retained behavior contracts; 69965 remain uncovered.
The qualification gate remains false.

## Validation and limits

Workspace/all-target checks, 595 runtime tests and the focused latest-Node public
CJS wrapper/file-URL regressions pass. The broader retained Node suite has five
pre-existing failures (173/178 pass), and shared regressions have one (3/4 pass).
`preexisting-node-comparison.json` retains reduced baseline/candidate comparisons;
one TLS test also fails on Node itself. These are not reported as green suites.

The all-eight instruction guard compares separate Cargo-built integration and
candidate executables with identical iteration profile/link settings. It uses
`evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh`, subtracts N=0 startup
from fixed N work and retains all 32 counts plus independent completion checks.
The RSS snapshot guard's largest per-run increase is +0.61% (DeltaBlue), below the
1% hold threshold. This is a regression screen, not a production performance win.
Host/build metadata, executable hashes and fixture hashes accompany the reports.
The final build uses integration 053afbcff and candidate 83242b53a. Its baseline
and candidate are respectively byte-identical to the 9bfffee76/8a1cc805c pair:
ld.lld is absent on this host, so the new symbol-order file is not applied.
The final guard has all eight workloads below the hold threshold, with a largest
per-run increase of +0.62% (DeltaBlue); all 32 completion checks pass. The build
metadata records this limitation and the equality hashes explicitly.
Interrupted reports are partial checkpoint evidence, never complete suite results.

Shared capability blockers include cyclic ESM/live bindings, require(ESM), Worker
and shared memory, JavaScript WebAssembly, HTTP2, N-API, Module construction,
btoa/atob, and foreign-program exception handling. Minimal module-catch and ESM
cycle reproductions are retained with the fixtures. Production fixes use shared
runtime/host behavior, with no package-name bypasses or fabricated success.
