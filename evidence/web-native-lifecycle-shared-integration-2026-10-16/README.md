# Native lifecycle shared integration

Published source f4e6ed305cffd9e33515090ecabd9b25134fe169 passes 392 serial Node v26.12.0 API comparisons (224.09 seconds), 671 Runtime tests (75.75 seconds), workspace/all-target checks, 43 framework fixtures and all nine no-JIT web correctness targets. Before/after manifests are identical: tracked source files, Node oracle, frozen CLI, mutable host CLI, runner and API-test binary. Git blobs verify the recorded source hashes. Later source is excluded.

The shared integration preserves both owners' appended tests and source policies, including the directory SDK and caller/protocol work. Its proof is independent of the earlier 387-case lifecycle snapshot. Runtime tests ran alongside compilation and a separate frozen executable's instruction profiler; no wall-time comparison is claimed. Full APIs ran serially after the profiler and heavy builds finished.

This is bounded compatibility evidence. Total stable Node parity remains open. The Express JSON load diagnostic still has request timeouts; no production throughput, RSS qualification or benchmark win is established. Earlier failed HTTP integration and instruction regressions remain separately retained.
