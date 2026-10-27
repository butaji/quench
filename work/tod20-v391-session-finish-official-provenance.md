# TOD-20 v391 official Node parallel census

- Timestamp completed: 2026-10-27 01:48 UTC
- Branch/source: `v2-cloud-node` / `48d838813316c263ae6686882c928b3428c12caf`
- Fixture inventory: Node v26.12.1 official source, 5,021 fixtures, inventory hash `aa0d2d758e3809c7`; root `/tmp/tod20-node-v26.12.1-official-source/test/parallel`
- Runner: `/tmp/tod20-v164-session-target/release/run-parallel`, SHA-256 `021e348d64c490d3f0d6771e2290789c7403a1b075f33e70e7d719d6391d5a1c`
- Quench child executable: `/tmp/tod20-v164-session-target/release/quench-node`, SHA-256 `52245fe311842988869d9c4216a6f7ef2f91bcf612460b147601ccd4ea4f0dc6`
- Oracle Node executable: `/workspace/work/node-v26.12.1-linux-x64/bin/node`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`
- Runner configuration: 5 jobs, 60-second per-fixture deadline, resumed from preserved append-only checkpoint after parent interruption; same source, runner, executable, fixture root, and environment throughout.
- Terminal accounting: 3,407 PASS / 651 TAP SKIP / 776 FAIL / 179 TIMEOUT / 8 CRASH / 0 UNCLASSIFIED = 5,021. All 5,021 result paths are unique and match the 5,021 unique checkpoint paths; all rows carry the expected inventory hash. All 651 skips are verified `1..0 # Skipped:` TAP output. Runner terminal summary agrees with the result JSON.
- Gate delta: 593 PASS below the required 4,000. Do not add focused cohort results to this official count.
- Matched v386 baseline: 3,408 PASS / 650 TAP SKIP / 778 FAIL / 176 TIMEOUT / 9 CRASH / 0 UNCLASSIFIED. v391 transition: -1 PASS, +1 SKIP, -2 FAIL, +3 TIMEOUT, -1 CRASH.

Compressed terminal evidence in this directory:
- `tod20-v391-session-finish-official-results.json.gz` (raw SHA-256 `e11710aa02b41595ff292fcf44948da3da1e866d8d8048c86add078ca61dddc0`)
- `tod20-v391-session-finish-official-checkpoint.jsonl.gz` (raw SHA-256 `0f405353ec24323b060f252b9620b1be2f4287431e1dce4a2e428536f979386b`)
- `tod20-v391-session-finish-official-run.log.gz` (raw SHA-256 `1f26708c55342e866d600f95f41ae613e585646fdfa7f4ab12d4de94126a0b63`)
