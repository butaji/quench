# Transfer and filesystem metadata parity qualification

Transfer repair source: `003be24ed`. Official oracle: Node v26.12.0.

The seven structured-clone differential comparisons cover public transfer markers,
proxy rejection without traps, wrapper function descriptors, serialized graph aliases,
getters that resize/write/detach backing stores, transfer commit order and partial
detachment on detach-key failure, and post-commit typed-view deserialization failures.
The complete API suite passes 259/259 serially. Runtime tests pass 619/619 and the
workspace/all-target check passes. All nine Node/Bun/Quench framework correctness
combinations pass with JIT disabled. Correctness mode does not measure throughput.

Source `3baa754c2` additionally routes stat/lstat/statfs through the native file URL
converter. Its focused Node differential test and framework-core 43/43 pass. These
results do not retarget the earlier transfer runtime/API or instruction snapshot.
The oracle harness CWD fix is source `a96010ed1` and has a focused harness check.

The instruction guard measures the frozen transfer binary, with the identical
baseline binary and input paths reused from the earlier clone qualification.
Raw counts and hashes identify the exact artifacts. Iteration instruction counts
are not production throughput, RSS, or isolated repair attribution. Earlier
startup/instruction regressions in TOD-135 remain open against their own baselines.

`fs-callback-probe` records an additional confirmed discrepancy, before its repair:
metadata APIs invoke a throwing callback twice in Quench versus once in Node.
It is retained as an open finding until the later repair is independently checked.

Total Node parity remains open, including shared backing-store cloning, host object
clone protocols, workers, HTTP/2 transport and broad Node/npm behavior. No benchmark
win or optimization claim follows from this milestone.

Integration source `aaaa5eb7d` passes 622 runtime tests and seven clone comparisons.
These integration results do not qualify subsequent URL/callback/URI repairs.
