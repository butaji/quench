# Splay record candidate: direct legacy Cell slots (M4)

Candidate source commit: `e7a5d9584447c553c4c295b2348036f4d669b0af`, based on rejected record-space candidate `63fc6629bc81c7bc44b2938ae2bc553e3395a016`.

This slice retains direct ObjectRecord Values and returns every legacy Cell payload to its original SlotArena slot. It removes the `SlotEntry` plus `RecordArena<Cell>` indirection. The same slot-owned Cell is used by accessors, marking, weak pruning, sweep, property compaction, and memory profiling.

Correctness checks on Apple M4 arm64 macOS:

- `cargo check -p quench-runtime`: passed.
- `cargo check -p quench-runtime --features profile-memory,profile-aggregate,profile-gc-time`: passed.
- `cargo test -p quench-runtime`: 551 unit tests and 6 integration tests passed.
- Full pinned Wasm testsuite, submodule `67c8b9dc32c47d61c05a1681df349d01c85fa8ff`: 67,124/67,124 passed; inventory complete. Raw report: [`wasm-shared.json`](wasm-shared.json), SHA-256 `6bae5d9f85b94f879ddffdd796e3721a3efbc24e24ec395235b2a77f96ef6c18`.

## Splay fixed-work screen (Apple M4 arm64 macOS)

The schema-5 Splay A/A and A/B ran for 300 `run()` calls per process against the pinned current-trunk production binary. Raw reports:

- [`splay-aa-schema5-11.json`](splay-aa-schema5-11.json): trunk binary against itself, 11/11 clean pairs; work max RSS median 116,375,552 B and setup-only 65,454,080 B.
- [`splay-ab-schema5-11.json`](splay-ab-schema5-11.json): trunk SHA-256 `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd` versus candidate SHA-256 `e86118d57ad11cfe4d3c2fe919037b51ded3da1af70198d05071d985aae84b7d`; output equal, 10/11 clean pairs. The one excluded pair failed the detector's effective-cycle-rate check (ratio 0.178 versus minimum 0.9).

Using only clean pairs, paired marginal wall time changed −5.425% (95% bootstrap interval −6.506% to −4.353%), cycles −5.522% (−6.175% to −4.939%), and retired instructions −3.819% (−3.837% to −3.802%). Median work max RSS fell from 116,375,552 B to 87,703,552 B (−24.63%); setup-only max RSS fell from 65,437,696 B to 50,429,952 B (−22.93%).

This is a Splay-only fixed-work result, not a stock-harness qualification or an all-eight guard. The official M4 distances therefore remain the last qualified trunk values: 1.664× Score and 1.810× max RSS. Next gate: all-eight A/A and candidate A/B against these exact pinned binaries, with clean cycles required for this layout change. Re-take the stock Splay campaign only after that guard passes.
