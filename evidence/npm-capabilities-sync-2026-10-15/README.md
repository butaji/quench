# npm repair sync qualification, 2026-10-15

This refresh includes integration cb077bb75 and retains the repair batch described
in ../npm-capabilities-2026-10-15. Latest Node v26.11.1 scenarios remain 260 API
passes, 13 declaration passes, 26 Quench failures and one platform oracle failure.
The full-function gate remains false: 336 observed paths have passing contracts,
69,900 of 70,236 paths remain uncovered. Descriptor matches do not prove behavior.

The intervening compiler merge df66a029f caused two real regressions: jiti and
TypeScript project-service lost parity. Wide instruction headers expose the Wide
marker; target protection/relocation and coalescing boundaries must first decode
the actual opcode. dd3ddf046 routes those decisions through fixed_width, retaining
wide-function rewrites and liveness. Two compiler regressions protect jump targets,
relocate them after compaction and forbid coalescing across a wide conditional.
The independent large-function reproducer segfaults on the pre-fix binary; Node
and the repaired binary print [0,18000]. Probe reports preserve the causal checks:
disabling forward move sources alone does not fix the packages, while suppressing
wide-function or pair rewrites does. The final shared repair restores both.

The RSS merge cb077bb75 adds lazy DOM events, process flags and console formatting,
and retrains symbol orders. This host lacks ld.lld. The lazy loader must use original
formatter identities when public util exports are replaced, including replacement
before the first formatted output. 40060bac0 retains a private formatter projection
in the existing UtilIntrinsics facts, without exposing extra public properties.
Lazy construction and the string-only path remain. The strengthened ESM Node test
passes; six other focused Node fixtures and 606 runtime tests pass. Workspace/all-
target checks pass. No whole-Node-suite or full-function parity claim is made.

The final all-eight instruction screen and independent 32 completion checks are
retained with exact binary/source/fixture hashes. Per-run measurements include the
upstream compiler changes and are not an isolated attribution or production speed
claim; startup costs are shown separately. Earlier completed snapshots preserve
history and are not substituted for this final binary's checks. Remaining cyclic
ESM, require(ESM), Worker/WebAssembly/N-API, HTTP2/HTTPS, parseArgs, stream/zlib and
stack discovery obligations remain open in Linear TOD-127/TOD-128/TOD-129.
