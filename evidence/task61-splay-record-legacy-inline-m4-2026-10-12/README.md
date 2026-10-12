# Splay record candidate: direct legacy Cell slots (M4)

Candidate source commit: `e7a5d9584447c553c4c295b2348036f4d669b0af`, based on rejected record-space candidate `63fc6629bc81c7bc44b2938ae2bc553e3395a016`.

This slice retains direct ObjectRecord Values and returns every legacy Cell payload to its original SlotArena slot. It removes the `SlotEntry` plus `RecordArena<Cell>` indirection. The same slot-owned Cell is used by accessors, marking, weak pruning, sweep, property compaction, and memory profiling.

Correctness checks on Apple M4 arm64 macOS:

- `cargo check -p quench-runtime`: passed.
- `cargo check -p quench-runtime --features profile-memory,profile-aggregate,profile-gc-time`: passed.
- `cargo test -p quench-runtime`: 551 unit tests and 6 integration tests passed.
- Full pinned Wasm testsuite, submodule `67c8b9dc32c47d61c05a1681df349d01c85fa8ff`: 67,124/67,124 passed; inventory complete. Raw report: [`wasm-shared.json`](wasm-shared.json), SHA-256 `6bae5d9f85b94f879ddffdd796e3721a3efbc24e24ec395235b2a77f96ef6c18`.

No performance measurements have been made on this candidate yet. Splay and all-eight schema-5 fixed-work gates against the pinned current-trunk binary remain pending a quiet host window.
