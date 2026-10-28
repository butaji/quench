# Raw filesystem metadata path and typed-view parity

Production repair source `cd4327788`; test expectation correction `4a952c16c`.
Oracle: official Node v26.12.0. The frozen repair executable passes all267 serial
API comparisons, all24 watch comparisons, framework-core43/43 and all9 no-JIT
framework correctness combinations. The corrected shared runtime suite passes623
and workspace/all-target checks pass. This is a functional checkpoint, not total
Node parity or a web throughput win.

Node path validation and byte conversion have one Rust owner. stat/lstat/statfs now
accept intrinsic Uint8Array paths, preserve raw host filenames and subview bounds,
bypass public Buffer/view hooks, reject NUL paths with Node's validation errors,
and consume path bytes after option getters. Error paths retain the original input
text. stat and lstat retain Node's distinct missing-entry suppression policies.
Immediate relative metadata I/O uses the retained working-directory inode, including
after directory rename/unlink. Watch registries share validation while retaining
lexical absolute keys rather than exposing descriptor paths as registry identity.

The typed-array validator now shares method diagnostics with iterator creation.
The installation table owns method names; diagnostic lookups occur only on error
paths. Brand, detached and out-of-bounds errors and argument ordering are compared
against Node for ten methods. Runtime tests exercise ordinary and specialized code.

The raw-path check uses non-UTF8 filenames seeded directly through Rust filesystem
I/O, not only matching two wrappers' path coercion. It also checks original error
metadata, option-driven Buffer mutations, poisoned public view properties and actual
renamed/unlinked directory effects. Initial failed checks are retained: the byte-copy
API represented detached views as empty, iterator diagnostics were separate, and the
runtime test initially expected #<Object> for an object with its own toString method.
The latter was an incorrect test expectation; Node and Quench both report
[object Object]. The correction changes no production implementation. Those failed
development logs are not qualifications of the final snapshot.

All-eight instruction guard raw counts and binary hashes identify the frozen repair.
The unchanged baseline binary and input paths are reused. Counts include shared-lane
changes and are from the iteration profile, not isolated repair attribution,
production throughput or RSS. TOD-135 retains the regressions and earlier baselines.

`fs-byte-path-probe` records the before-repair discrepancy. `statfs-presentation-probe`
records the next open finding: values have the correct primitive kind, but Quench's
statfs return prototype is Object rather than Node's canonical StatFs class. Async
path snapshot semantics, read/write path families and broad Node APIs also remain
open. The user's parity-before-optimization prerequisite remains in force.
