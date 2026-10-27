# TOD-20 v393 focused HTTP/2 repeat

- Source: `v2-cloud-node` commit `7688fcb79c1c5d75de5e6d08a20c6cc97b64be4e` (same runtime source as v392 full census; no runtime edits during repeat).
- Inventory: Node v26.12.1 official `test/parallel`, HTTP/2 filename filter, 284 fixtures, filtered inventory hash `3a28b8d6149a3e8d`.
- Configuration: 5 workers, 60s per fixture.
- Current outcome: 168 PASS / 72 FAIL / 42 TIMEOUT / 1 CRASH / 1 TAP SKIP; 284 unique paths, one skip confirmed by `1..0 # Skipped:` marker.
- Against the v392 full-run HTTP/2 subset: 168 PASS→PASS, 71 FAIL→FAIL, 40 TIMEOUT→TIMEOUT, 1 CRASH→CRASH, 1 SKIP→SKIP, 1 CRASH→TIMEOUT, 1 PASS→TIMEOUT, 1 PASS→FAIL. This repeat confirms several volatile outcomes; it is not full-gate credit.
- Against v391’s HTTP/2 subset: v391 had 170 PASS / 69 FAIL / 43 TIMEOUT / 1 CRASH / 1 SKIP; v392 full had 170 / 71 / 40 / 2 / 1; this repeat had 168 / 72 / 42 / 1 / 1. The old/new runner hashes differ, so transitions are descriptive rather than attributed to runtime source.
- Runner SHA-256 `be10dce68c2f350dcb4acd0f8a07d767241b3065effdd0bd60625922962f43b3`.
- Child Quench CLI SHA-256 `1f7e20c2a9f695c26be9d06da77dfd30f4e51907eb9643b0431f2aec11f5cbf7`.
- Node executable SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Result JSON SHA-256 `0036737a489ee066330d159dfa444d428150f80fbd24ad0e17f4b33652620963`.
- Checkpoint JSONL SHA-256 `3bef8cf753a91f7729e37330c0d6480080b31e3d5e83f84b257a23703ae239f4`.
