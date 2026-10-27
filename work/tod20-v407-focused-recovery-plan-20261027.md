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
  `bf8ac113d4902c8f5c4ccbb66850478f0d67ed4aacddde06bfde59c5b418d4d3`.

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

First collect a matched v406 family cohort with the same runner and fixture
root; the two one-fixture JSON files above cannot establish regression
transitions for the rest of either family. Then require no attributed
pass-to-fail/timeout/crash transition in the selected controls.

1. VM core: `test-vm-module-import-meta.js`,
   `test-vm-module-reevaluate.js`, `test-vm-module-link-shared-deps.js`, and
   `test-vm-module-evaluate-source-text-module.js`. The
   `test-vm-module-after-evaluate.js` file exercises a separate microtask queue
   issue and is not a focused control for this patch.
2. VM assertions not covered by those fixtures: ordinary file-ESM
   `import.meta.resolve`, absent-callback defaults/prototype/own keys, exact
   callback receiver and module identity, duplicate public identifiers with
   distinct callbacks, callback throw and errored-module behavior. Add a
   custom-identifier stack assertion: an error from
   `new SourceTextModule('throw new Error("x")', { identifier: 'custom-name' })`
   should retain the caller identifier in its frame. A Node 24.19 read-only
   oracle showed `at custom-name:1:7`; confirm formatting with Node v26.12.1.
   Add a path-like explicit VM identifier and an ordinary file-ESM stack control
   to verify only the VM identifier path uses the verbatim mapping.
   Keep TLA and mixed-source graphs outside the claim until they use a native
   async module path.
3. VFS: `test-vfs-require.js` and `test-vfs-import.mjs`; the latter has no
   extensionless case and is only an ordinary VFS ESM control. Add targeted
   VFS probes for an exact extensionless CommonJS file, nested `type: commonjs`
   under a module package, module-scope `.js`/`.mjs` siblings, and
   extensionless directory/package-main resolution. Use a separate ordinary
   filesystem extensionless probe; the VFS fixtures do not cover it.
4. Callback-error-routing fallback: do not spend a full cohort on this
   defensive patch unless a targeted test constructs
   `RootedError.exception == None` with `JsError::thrown_value() == Some` and
   verifies identity, handler routing, formatting and root release. Existing
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
  required. In other preserved v406 full-suite runs, this fixture failed earlier
  at line 19 (CJS require) or line 242 (VFS path handling); the single-fixture
  line-379 failure is one reproduction, not proof those separate failures are
  fixed.
- Stack formatting must use the public SourceTextModule identifier verbatim.
  The candidate change in `shared_vm.rs::stack_source_name` now returns that
  identifier directly for mapped VM modules; an independent Node 24.19 oracle
  observed `at custom-name:1:7`, but Node v26.12.1 comparison and Quench
  behavior validation remain pending. Normal file ESM continues through the
  original URL formatting path.
- The callback-error-routing fallback remains unproven: reviewed production
  constructors already root thrown JS values, and no caller demonstrating
  `exception == None` with a thrown value was found. Do not claim it as a fix
  without a reproducer covering exception identity, listener routing, final
  formatting, and root release.

## Current execution constraint

The latest independent mount/cgroup audit found zero bytes available on the
`/workspace` overlay and 83,824,640 bytes free on `/tmp`. Existing Quench
executables are 84–152 MB, and no existing rlib/fingerprint matches this
worktree; 152 MB is only a lower bound to materialize one comparable executable,
so `/tmp` is short at least 68 MB before build intermediates. Cgroup memory
samples ranged from 17,157,578,752 to 17,178,279,936 of 17,179,869,184 bytes
(about 21 MiB to 1.5 MiB headroom), with no swap and OOM counters 551 / 166.
`/dev/shm` has 9,441,337,344 nominal bytes free but is charged to this same
cgroup. No build or test process is live. A prior rustc reached about 1.1 GB
RSS. Do not start another fixture, build or link at this headroom. The six CLI
originals and all census evidence remain preserved; the denied archive/removal
operation is not authorized by this document.

## Next reproduced shared-root experiment: promise FileHandle identity

Read-only comparison of the three persistent v386/v406 failures confirms one
specific cause. Node 26.12.1's `internal/fs/promises.js` defines a private-brand
`FileHandle`; `fs/promises.open()` constructs it from the raw owner returned by
`internalBinding('fs').openFileHandle`. The three aggregate/op/close error
fixtures replace `FileHandle.prototype.fd` with a throwing getter and verify
that high-level `readFile`, `writeFile`, and `truncate` paths observe it and
close the handle. A prototype-only shim cannot pass Node's private brand.

Quench's `fs_shared_vm.rs::PROMISES_FACTORY.open` currently calls `openSync`
and returns a plain object with its own data `fd` and closure-based operations.
That own field shadows the canonical prototype getter. The bootstrap
`__quenchInternalFsBinding.openFileHandle` returns `undefined` whenever Node
passes the promise request, and the CommonJS fallback only implements a
synchronous leak diagnostic. This makes a descriptor-backed raw owner the
missing generic adapter, rather than another test-specific getter workaround.

After a safe candidate build becomes possible, test the smallest complete
adapter seam: provide promise-mode `openFileHandle` with one descriptor owner
implementing `fd`, `close`, `closeSync`, and `getAsyncId`, then create handles
with the canonical cached `FileHandle` constructor. Ensure `readFile`,
`writeFile`, and `truncate` read the live `handle.fd` and call the live
`handle.close`, retaining one descriptor owner and Node's error precedence.
Do not fake the private brand with `Object.setPrototypeOf`. Confirm Node's
aggregate order is `[operationError, closeError]` and that a getter failure
causes one close attempt before coding the rejection shape.

The first focused cohort is `test-fs-promises-file-handle-op-errors.js`,
`test-fs-promises-file-handle-close-errors.js`, and
`test-fs-promises-file-handle-aggregate-errors.js`, plus controls for ordinary
`open` brand/prototype/`FileHandle.isFileHandle`, close once/idempotence,
`test-fs-promises-readfile.js`,
`test-fs-promises-readfile-buffer-option.js`, and
`test-fs-promises-file-handle-read-worker.js`. Include an explicit getter
throw/close override check for getter count, close count, and error identity.
No implementation or candidate result exists yet; these unchanged v406/v386
failures cannot be counted as candidate progress. Constructor export visibility
and initialization-cycle safety remain the first design probe.

## Constructor initialization probe

Source inspection found an existing lazy pattern that may avoid the cycle:
`initialize_promises_glob_policy` loads `internal/fs/promises.js` through
`commonjs::node_library_module` only after the public `fs` module has been
cached. Its CommonJS loader returns the module record, whose official Node
source exposes `FileHandle` alongside `exports`. A candidate can likely defer
obtaining that cached constructor until the first `open()` call, instead of
loading it while `fs_shared_vm` is constructing `fs.promises`. This is a
design path, not verified behavior: confirm which module record shape Quench
returns and that the internal module's binding initialization remains safe
before relying on it. The fallback CommonJS binding path and startup ordering
must be kept consistent with the primary `internal_fs_binding.rs` adapter.

Other reviewed clusters remain lower priority until a single root is sharper:
the HTTP/1+HTTP/2 socket-handoff theory is medium-low confidence and spans
different protocol failures; nested TLS is an exact two-fixture root but needs
layered transport composition; Abort dependent cleanup is an exact small
cluster; worker IPC requires a main-thread route/ack protocol, not a shallow
export. These counts remain diagnostic and do not replace the official full
census gate.

## Fresh process and resource checkpoint (2026-10-27 22:41 UTC)

No live cargo, rustc, Quench, Node, or `run-parallel` process exists; matching
historical runner and fixture children are zombies only. The frozen v406 CLI
and runner still hash to `7ddaef7181aba0a97dfc2e06dfc5305d5e218766eb8462d9b6f1505756d375ad`
and `9385e8714a3867413f32a117878472c212f9b6e415c1d0f045974f3b2946677a`.
The current five-file source diff remains
`bf8ac113d4902c8f5c4ccbb66850478f0d67ed4aacddde06bfde59c5b418d4d3` and
passes `git diff --check`; this does not validate runtime behavior. The overlay
still has zero free bytes and `/tmp` has 83,824,640 bytes. Cgroup memory is
16,981,970,944 / 17,179,869,184 bytes (about 189 MiB headroom); OOM counters
remain 551 / 166. `/dev/shm` is empty, but another link would compete for the
same cgroup memory and `/tmp` lacks room for the known executable alone plus
build intermediates. No build, candidate test, archive, move, or deletion was
started.
