# SQLite Wasm reference-body-cache A/B

This extends the existing three-round same-artifact reference-body-cache screen to the call-heavy `sqlite3-wasm` fixture. The Quench runtime executable was identical in both arms; only `QUENCH_WASM_JIT_REFERENCE_BODY_CACHE` differed. Output matched in all three rounds.

The off/on Quench median Score was 0.32 / 0.30. The paired on/off score ratio was 0.897, with a 95% interval of 0.841–0.938, indicating the cache slowed this fixture. Median peak RSS moved from 170,315,776 to 169,668,608 bytes; the paired ratio was 0.9962 (95% interval 0.9928–0.9985), a small memory reduction. Node scored 39.15 and Bun 48.36 in this screen.

The cache stays opt-in. This is a three-round diagnostic screen, not qualification. The benchmark report records a dirty source tree because the bench CLI's supported-fixture validation was extended for this run; the runtime executable was the same production binary in both arms. Raw report and analysis are retained here.
