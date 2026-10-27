# TOD-20 focused recovery gate (2026-10-27)

This is a test plan and baseline reproduction record. It does not validate or
credit the unbuilt five-file worktree diff.

## Frozen baseline and environment

- Official result baseline: v406, 3,415 PASS / 651 TAP SKIP / 761 FAIL /
  183 TIMEOUT / 11 CRASH / 0 UNCLASSIFIED of 5,021; goal remains 4,000 PASS.
- Oracle: Node v26.12.1, official physical source root
  `/tmp/tod20-node-v26.12.1-official-source`, official inventory hash
  `aa0d2d758e3809c7`.
- Preserved CLI: `/tmp/tod20-v406-quench-node`, SHA-256
  `7ddaef7181aba0a97dfc2e06dfc5305d5e218766eb8462d9b6f1505756d375ad`.
- Preserved runner: `/tmp/tod20-v164-session-target/release/run-parallel`,
  SHA-256
  `9385e8714a3867413f32a117878472c212f9b6e415c1d0f045974f3b2946677a`.
- These binaries report runtime commit `967bdc3f18982122e0cc6f5406c82266fc4430a2`,
  but predate the current uncommitted source diff. Never use their results as
  candidate credit.

## Exact baseline reproductions

Both runs used one worker, the official fixture root and 60-second deadlines.
Both machine records contain exactly one FAIL, zero skip/timeout/crash/
unclassified, and Node v26.12.1.

| Fixture | Baseline failure | Inventory / result SHA-256 |
| --- | --- | --- |
| `test-vm-module-import-meta.js` | Line 22: `undefined !== 42`; `initializeImportMeta` does not populate `meta.prop`. | `d28fd4f3aa03dc2e` / `0b6adbbb48ab2a00f85ee102e93aee76577cc72ee1bad768841861835ccd9ef5` |
| `test-vfs-require.js` | Line 379 dynamic import of `/esm-pkg/entry` fails with `Not a directory (os error 20)` before the extensionless VFS module loads. | `8e19855b87364cb0` / `906bac51c9fba28584eebf658f6623d5fa8bdf9a9549a0a07732eda15d0496bf` |

Result JSON is preserved at `/tmp/tod20-v406-vm-import-meta-single.json` and
`/tmp/tod20-v406-vfs-require-single.json`.

## Candidate focused gate (run only after a safe candidate build exists)

Use the preserved runner with `QUENCH_NODE_TEST_EXECUTABLE` set to the newly
built candidate CLI. Keep the official root, one job and 60-second case
deadlines. Capture machine JSON and preserve the exact candidate CLI, runner,
source-diff, fixture-inventory and result hashes.

1. VM metadata positives and lifecycle: `test-vm-module-import-meta.js`,
   `test-vm-module-after-evaluate.js`, `test-vm-module-reevaluate.js`,
   `test-vm-module-link-shared-deps.js`, and
   `test-vm-module-evaluate-source-text-module.js`.
2. VM regressions: ordinary file-ESM `import.meta.resolve`, absent callback
   defaults/prototype/own keys, exact callback receiver and module identity,
   duplicate public identifiers with distinct callbacks, callback throw and
   module errored-state behavior. Keep TLA and mixed-source graphs explicitly
   outside the claimed pass set until they use a native async module path.
3. VFS positive and controls: full `test-vfs-require.js` and
   `test-vfs-import.mjs`; then verify CJS extensionless fallback and ordinary
   filesystem extensionless resolution remain unchanged.
4. Callback-error-routing fallback: do not spend a full cohort on the current
   defensive patch unless a targeted test can construct
   `RootedError.exception == None` with `JsError::thrown_value() == Some` and
   verify identity, handler routing, formatting and root release. Existing
   production callers reviewed so far do not demonstrate that state.

Only after the focused positive/regression gate and build provenance pass,
rebuild and rerun the unchanged full 5,021-fixture census. Report all six
classifications and transitions; focused results never count toward 4,000.

## Current execution constraint

After the two single-fixture baseline runs, `/tmp` has 83,836,928 bytes free.
Cgroup memory is 17,153,998,848 / 17,179,869,184 bytes, leaving about 25 MiB;
OOM counters remain 551 / 166. No build or test process is live. The required
release link was estimated to need about 200 MB free and Rust compilation has
previously reached about 1.1 GB RSS. Do not start another fixture, build or
link at this headroom. The six CLI originals and all census evidence remain
preserved; the denied archive/removal operation is not authorized by this
document.
