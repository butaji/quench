# Array storage branch diagnosis

Platform: M4 arm64 macOS. This is static candidate evidence, not a performance result.

The candidate source is based on `883e5c00f`; trunk `4a678f396` adds evidence-only commits after that source. The earlier single-space candidate was rejected by the all-eight gate. Its Splay-only diagnostic was 1.20x speed / 1.66x RSS, but those distances do not apply to this revised candidate.

The original candidate's dense `SetIndex` path called the generic `Object::has_indexed_descriptors()`. That reached `Object::extras()`, which inspected the `ARRAY_ELEMENTS` storage-mode bit before selecting the array union arm. The revised path calls `Cell::array_has_indexed_descriptors()`, so the already-matched `Cell::Array` variant reads the fixed `ArrayStorage.extras` option directly. Array extras no longer use the generic object-extras bit; the option is their presence authority.

The optimized profiling binary was SHA-256 `5da04b281617064b3adf641f020ac5c3e44670ce0365252b8f69bee1101a4052`. In `set_index_mode`, the dense-store preamble at `0x1002a5b5c` checks the cell kind, reads the fixed extras pointer, checks indexed-descriptor and frozen state, then loads the element vector and indexed value. It contains no test of `ARRAY_ELEMENTS` (bit 29). The preceding build had a `tbnz` of bit 29 in this preamble. This verifies removal of that added storage-mode test from the common store path; it does not show that the test alone caused the fixture instruction regressions.

Correctness checks after the change:

- `cargo test -p quench-runtime --lib heap::cell::cell_layout_tests`: 3 passed.
- `cargo test -p quench-runtime --lib indexed_write`: 3 passed.
- `rustfmt --edition 2024` on `heap/cell.rs` and `vm/index.rs`: passed.
- `git diff --check`: passed.

No Splay or all-eight timing was run. At inspection, host load was 26.73 / 19.40 / 17.39 with Deno workers using the CPUs; timing remains pending a clean window. The revised candidate's speed and RSS distances are therefore unmeasured. The clean-trunk M4 baseline remains 1.24x speed / 2.09x RSS.
