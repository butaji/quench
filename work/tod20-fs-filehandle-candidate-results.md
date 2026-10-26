# TOD-20 isolated FS FileHandle candidate result

Date: 2026-10-26 21:20 UTC

## Exact inputs

- Candidate source base: `ef64d9b235962b17ab88005d79bf8439b5039208`.
- Changed `fs_shared_vm.rs` SHA-256: `ad1968c561bd9a9b3b45d1432e2a693cce1a187629fc014dea94eb58ca5a4170`.
- Quench child SHA-256: `8aa870671f5eff0cb320cceff3bab56e6e2e3d7e9173764ae836a694951791e2`.
- `run-parallel` SHA-256: `39a8b05082460a6a75901552b04e513b4ce92e4ca49bf533b9f56004ee826f6c`.
- Oracle: official Node v26.12.1 executable SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Fixture root: `/tmp/tod20-node-v26.12.1-official-source/test/parallel`, physical official Node v26.12.1 `lib/`; complete inventory hash `aa0d2d758e3809c7`.
- Build target: `/tmp/tod20-v164-session-target`, release profile, two cargo jobs. `tests/node` temporarily used `/tmp/tod20-v26-node-compat-source`: official Node v26.12.1 `lib/`, pinned submodule `src/` and `deps/` at `26e72d85a611e222e3f06025de8a3c496923bacd`; source header SHA-256 `971263ecd7bf9534c80ab1c69bf4df1643d5c9af1a913bcdea85d6b39589c92c`. The original `/workspace/quench/tests/node` symlink was restored immediately after build.

## Test outcome

Command filter `test-fs-promises-`, five workers, 60-second deadlines. The 36-case candidate cohort is **30 pass / 0 skip / 6 fail / 0 timeout / 0 crash / 0 unclassified**. Matched v386 outcomes for those same 36 official fixture names are identical: 30 pass and 6 fail, with no transitions. All six failing fixture names are:

- `test-fs-promises-file-handle-aggregate-errors.js`
- `test-fs-promises-file-handle-close-errors.js`
- `test-fs-promises-file-handle-op-errors.js`
- `test-fs-promises-file-handle-read-worker.js`
- `test-fs-promises-readfile-buffer-option.js`
- `test-fs-promises-readfile.js`

The three aggregate/close/op tests still report missing expected rejection after monkeypatching the internal `FileHandle` prototype. The candidate receives no pass credit and remains isolated. Result, JSONL checkpoint and log are preserved under `/tmp/tod20-fs-path-promise-fdclose-build/` with SHA-256:

- `fs-promises-candidate-results.json`: `fb86f02e542b5929118a98a250ef67a243f7813d40f243c3100031cc3c52e246`
- `fs-promises-candidate-checkpoint.jsonl`: `a261e43f2d24d9da699fbf42c694ac8c59a8962d728c1808cf1439b3851444ed`
- `fs-promises-candidate.log`: `094572c14e2fdc3e2d8c89351717aaa1d7dd4bcba21787c96a682ae60752f1e9`

The fixture subset inventory hash is `bd1bddaf8ed8f2f5`. The next step is source-only verification that the one-time setter injects the same constructor object exposed by `require('internal/fs/promises').FileHandle` and that the overridden descriptor is read on the custom slow path. Do not run a full `test-fs*` cohort until that seam is proven.
