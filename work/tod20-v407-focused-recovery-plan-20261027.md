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
- Current five-file production diff SHA-256:
  `5dcbbdff830a98f38a11f3720cfd8feaea7006e21d1cfa1d6767641b09f3b862`.

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

For each fixture below, invoke the runner in this form, changing `FILTER` and
`RESULT` so every observation has its own output file:

```sh
QUENCH_NODE_TEST_EXECUTABLE="$CANDIDATE_EXE" \
  /tmp/tod20-v164-session-target/release/run-parallel \
  --all \
  --parallel-dir /tmp/tod20-node-v26.12.1-official-source/test/parallel \
  --filter "$FILTER" --jobs 1 --timeout-secs 60 --results "$RESULT"
```

The selected exact fixture names are `test-vm-module-import-meta.js`,
`test-vm-module-after-evaluate.js`, `test-vm-module-reevaluate.js`,
`test-vm-module-link-shared-deps.js`,
`test-vm-module-evaluate-source-text-module.js`, `test-vfs-require.js`, and
`test-vfs-import.mjs`. The first and sixth must move from the reproduced v406
FAIL to PASS. The other fixtures are regression controls and must not lose any
baseline PASS status when compared with a same-runner/same-inventory v406
cohort; rerun any unstable transition before attributing it.

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

## Source-only correctness review (not runtime validation)

- Runtime `LoadImportMeta` allocates a null-prototype object, calls the host
  initializer before caching that object, and propagates callback errors. The
  candidate deletes only the host-added `url`, `filename`, `dirname`, and
  `resolve` own properties before calling user code; ordinary file ESM remains
  on the pre-existing path.
- Each VM wrapper has a unique NUL-prefixed internal runtime ID. The
  SourceTextModule wrapper keeps its public identifier for callback identity,
  and the host temporarily maps IDs for VM dynamic-import resolution and stack
  names. This covers duplicate public identifiers in the synchronous native
  graph; the pinned Node oracle had already confirmed duplicate identifiers
  with distinct callbacks.
- Callback and exact module roots are retained only for synchronous native
  graph evaluation; previous host maps are restored and retained roots are
  released on both returned success and error. The wrapper marks graph modules
  errored if that evaluation throws. The current TLA and mixed-source branches
  bypass this evaluator and remain outside the candidate claim.
- The VFS change requires `type: module` both at the extensionless request and
  resolved target. That should preserve extensionless CommonJS and ordinary
  filesystem fallback. The v406 ENOTDIR reproduction reaches the target
  scenario at test line 379, but this source-level review cannot establish that
  the candidate intercepts it correctly; the focused VFS fixtures above remain
  required.
- The callback-error-routing fallback remains unproven: reviewed production
  constructors already root thrown JS values, and no caller demonstrating
  `exception == None` with a thrown value was found. Do not claim it as a fix
  without a reproducer covering exception identity, listener routing, final
  formatting, and root release.

## Current execution constraint

After the two single-fixture baseline runs, `/tmp` has 83,836,928 bytes free.
The latest recheck found cgroup memory at 17,153,531,904 / 17,179,869,184
bytes, leaving about 25 MiB; OOM counters remain 551 / 166. The overlay backing
`/workspace` has zero available bytes. `/dev/shm` is a separate tmpfs with
9,441,337,344 nominal bytes free, but shares the same cgroup; `memory.stat`
shows about 9.35 GB of charged shmem. No build or test process is live. The required
release link was estimated to need about 200 MB free and Rust compilation has
previously reached about 1.1 GB RSS. Do not start another fixture, build or
link at this headroom. The six CLI originals and all census evidence remain
preserved; the denied archive/removal operation is not authorized by this
document.
