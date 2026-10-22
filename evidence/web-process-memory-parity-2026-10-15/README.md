# Live process memory and host-function metadata

Frozen source `4aa47e844` passed 290 serial stable-Node API differentials, 627 runtime tests, workspace/all-target checking, 43 framework fixtures and all nine no-JIT web correctness targets. The runtime run used identical production source; its additional Rust test was formatted before committing, without changing its JavaScript fixture or assertions.

On Linux, process.availableMemory() follows live cgroup limits/current usage and physical memory fallback; process.constrainedMemory() combines cgroup and inherited AS/DATA resource limits. The values no longer use the legacy placeholder. A single physical-memory snapshot supplies the OS facts. Data-limit and address-space probes both agree with stable Node's 12 GiB constraint. Available memory is dynamic, so the tests verify type/bounds and native data provenance rather than equality across separate processes.

Host callbacks now create length before name, preserving standard own-key ordering and descriptors for plain callbacks, captured callbacks and constructors. The shared Core test exercises both interpreter and specialized execution. These are functional changes, not an optimization claim.

All 32 fresh matched Callgrind executions completed valid fixed work with unchanged identities. DeltaBlue (+5.9233%) exceeds the 1% regression limit; the other seven pass, including Earley-Boyer (+0.6645%). No production throughput, RSS, isolated attribution or win claim is made. The separate full stable sweep is diagnostic and still running; its binary/source inventory is frozen independently. Later OS memory function/anonymous callback work is excluded from this candidate.
