# Native argument parsing and npm qualification

Total Node/npm parity is **not reached**. Package performance wins remain a later
phase, after parity; this packet makes no package speed claim.

Source `88abab792` implements `util.parseArgs` in Rust over captured shared-VM
intrinsics. Private VM data retains the original operations without restoring
mutated guest properties. The shared runtime owns strict property assignment
and startup intrinsic lookup; Node owns validation, tokenization and option
policy. Defaults retain identity, token arrays remain canonical VM state,
validation preserves UTF-16 labels, and the CLI normalizes eval/print arguments.

`verify.py NODE QUENCH STANDALONE_NODE_ROOT OUTPUT_DIR` checks the pinned upstream
fixture hash, all **117** upstream cases, **1,200** deterministic generated cases,
and focused identity/getter/token/frozen-intrinsic/strict-write/UTF-16 contracts.
Its upstream adapter replaces only test/suite scheduling with explicit sequential
completion counting, and relocates the two helper import paths. Original test
inputs and assertions remain. The standalone checkout prevents this repository's
`type:module` scope from changing Node's original CommonJS child fixture. Node
and Quench stdout/stderr match for all three retained corpora. The official
26.12.0 binary is the oracle; exact executable hashes are in snapshots.json.

Jackspeak's expanded contract has **71 assertions** over builders, parsing,
validation, config/environment effects, getters and renderers. The completed
source-equivalent 88abab792 package snapshot records **268 API passes, 13
declaration passes, 18 Quench failures and one musl/glibc Node failure**. Jackspeak
now passes, but lucide-react times out in this run. A focused recheck also times
out; clean integration3070816f4 passes its check while the previous606439b9e
binary times out. These separate timed observations do not isolate causation.
Discovery: **148 unqualified matches, 143 differences, nine oracle failures**.
The logical callable ledger projects contracts onto **397/70,236 observed paths**,
including aliases; **69,839** remain uncovered. Conditional entry-specific,
symbol, returned/dynamic API and all-input obligations remain open. No descriptor
match or contract corpus qualifies a whole package.

The 88abab792 Core passes **618 default-profile** and **618 iteration-profile**
unit tests; workspace/all-target checks pass. Matched opt1/non-incremental CLI
builds use isolated target directories for baseline3070816f4 and candidate.
The all-eight Callgrind screen has a maximum per-run delta **+0.00144%**, and
all **32** independently checked exact completions pass. Raw counts, derived
summary and full completion records are retained.

The broad API run against the exact frozen optimized candidate passes **250/250**
fixtures. Harness-only commit `7b4393d68` supplies an explicit Unix-exec CLI
selector, preserving PID, groups, descriptors and signals. Earlier debug-profile
parallel and serial runs were interrupted after deadline failures under compiler
and Callgrind contention. Their logs remain unqualified; a direct production
child-environment probe matches Node byte for byte. See broad-run-interruption.json.

Merge `2a42e5f04` includes integrationd1c09c900's HTTP repairs, without
changing Core source. Its **completed combined** qualification is **269 API
passes, 13 declaration passes, 17 Quench failures and one platform oracle
failure**; lucide-react passes this run. All **251 selected API fixtures pass**
against its exact optimized CLI. Discovery is **151 unqualified matches / 140
differences / nine oracle failures**; callable coverage remains **397/70,236**.
Workspace/all-target checks pass. Its fresh matched all-eight guard against
clean integrationd1c09c900 has max **+0.0145%**, with all **32** exact completions
passing. Post-sync reports/hashes/logs retain this snapshot separately.

The environment restarted after 193 scenario records. `resume-packages.py`
finished only the missing cases after verifying identical source/dirty state,
engine, inventory, lockfile, fixture and deadline identities. Historical rows
were retained; the final report contains exactly 300 unique packages.

The `file:` URL loader counterexample is separate from parseArgs. It remains
retained here; a subsequent isolated repair and its query/fragment identity tests
must carry their own qualification. Remaining major capabilities include
synchronous require(ESM), Workers, native Node-API, HTTPS/HTTP2, stream constructor
receiver initialization, zlib internals and async completion.
