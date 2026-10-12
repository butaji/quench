# Splay record candidate correctness gate

Host: Apple M4, macOS arm64. Candidate source: `675b3fe3db7b19e51d0be4b1c575b0fc5d58b59e`, based on `origin/v2` `55aab00d2`. The candidate contains the direct ordinary-object record space, restored direct legacy-cell access, the direct-object holder-cache fix, and removal of the now-unused ordinary-holder helper.

No benchmark timing was collected in this correctness gate. The current official Splay distances remain 1.664× Score and 1.810× max RSS from the prior stock run on trunk; this candidate has not been compared with that binary yet.

The M4 production candidate is built and pinned at `/Users/admin/Code/GitHub/quench-v2/target/pinned/afde285b7f8dff56da12f44eb915034b0599d7aec95a2ff39d55ae18a8d7c4cf/quench-node`, SHA-256 `afde285b7f8dff56da12f44eb915034b0599d7aec95a2ff39d55ae18a8d7c4cf`. It was built from this source at `11efad0ec7d7a4bf44885a07cc321627e79f017f` (evidence-only commit over the tested runtime source above).

Production build policy: `CARGO_PROFILE_PRODUCTION_LTO=thin`, `CARGO_PROFILE_PRODUCTION_CODEGEN_UNITS=1`, `RUSTFLAGS='-D warnings -C llvm-args=-align-all-functions=6 -C link-arg=-mmacosx-version-min=15.0'`, `CARGO_BUILD_JOBS=2`, `--profile production`. The captured final `quench-node` rustc command is in `candidate-rustc-command.txt`; it shows `-C opt-level=3`, `-C panic=abort`, `-C linker-plugin-lto`, `-C codegen-units=1`, symbol stripping, warning denial, alignment, and the macOS deployment target.

## Results

- `cargo test --locked -p quench-runtime`: 552 unit tests and 6 integration tests passed.
- `cargo check --locked -p quench-node`: passed with repository warning denial enabled.
- Full Wasm suite: 67,124 directives passed, 0 failed; inventory complete. Runner SHA-256: `e08767685adabfcf93bb1b6092fa27720478eefd6557eb552f12c4b33733d0a1`.
- Test262, pinned checkout `f2d1435644797268dca1f7988cad5a4e89ccd8d2`: 7,987 passed, 0 failed across stages 31, 47, 60–63, and 79–81. Stage reports record runner SHA, host, timeout and clean source revision.
- `git diff --check`: passed.

`cargo fmt --all -- --check` reports formatting differences across unrelated workspace and vendored OXC files; it made no edits. The candidate change itself is the removal of one obsolete helper and introduces no formatting changes.

## Reproduction

Run from the candidate worktree. The iteration target directory was reused from the repository target to avoid another dependency build.

```sh
CARGO_TARGET_DIR=/Users/admin/Code/GitHub/quench-v2/target/candidate-a CARGO_BUILD_JOBS=2 cargo test --locked -p quench-runtime
CARGO_TARGET_DIR=/Users/admin/Code/GitHub/quench-v2/target/candidate-a CARGO_BUILD_JOBS=2 cargo check --locked -p quench-node
WASM_FILE_TIMEOUT_MS=60000 CARGO_TARGET_DIR=/Users/admin/Code/GitHub/quench-v2/target/iteration CARGO_BUILD_JOBS=2 cargo run --locked --profile iteration -p quench-wasm-test --bin run -- --report <report.json> crates/quench-wasm-test/testsuite
TEST262_TEST_TIMEOUT_MS=900000 TEST262_JOBS=2 TEST262_REPORT=<report.json> CARGO_TARGET_DIR=/Users/admin/Code/GitHub/quench-v2/target/iteration CARGO_BUILD_JOBS=2 cargo run --locked --profile iteration -p quench-test262 --bin run-stages -- --root /Users/admin/Code/GitHub/quench-v2/tests/test262 --from <first-stage> --to <last-stage>
```

The raw Wasm and Test262 reports are stored beside this file.

The fixed-work tools already pinned for the M4 campaign are `quench-bench` SHA-256 `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109` and `task61-fixed-work-guard` SHA-256 `65c7d338250c899622b4fb5851964f7262602fbb09c5e6759f88196b1952a0c1`. The current-trunk comparison binary is `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`.

The same-binary all-eight A/A control is reused from the prior M4 campaign and copied here as `all-eight-aa-schema5-11.json` (SHA-256 `963a116fccdbec9f320e12c76d73443066dfbcb58a83c0f4afee6925d50fdb26`). It used the exact trunk executable SHA above, the same schema-5 runner SHA, and the same clean V8-v7 corpus submodule `64e1860c736c1b899708f4cd646721bc71e53d8b`; all eight fixtures had 11/11 clean pairs and equal output. Its report's `source_dirty` describes the source worktree from that earlier candidate session; the measured executable and runner are pinned by their SHA-256 values, and the corpus checkout is recorded clean. This is A/A noise-control evidence only, never a candidate result.

## Candidate fixed-work screen (M4)

Candidate binary SHA `afde285b…a8d7c4cf` vs trunk binary SHA `8432da11…bf5795dd`, runner SHA `bdc9d6c4…98db109`, corpus commit `64e1860c736c1b899708f4cd646721bc71e53d8b`. The all-eight pool has 12 clean paired rounds per fixture: seven from the resumable guard batches and five from `all-eight-ab-schema5-topup-5-wall-policy.json`. RegExp has 23 clean rounds after the additional `regexp-ab-schema5-wall-topup-11.json`. The seven-round guard reports are in `all-eight-ab-guard-topups/`; the checkpoint and its cycle-based decision are `all-eight-ab-guard.json` and `all-eight-ab-guard.decision.json`.

The initial one-round pilot `all-eight-ab-schema5-initial-1.json` had incomplete contention calibration and is excluded. An attempted 11-round all-eight report was stopped when six Deno workers drove one-minute load to 16.56; its Crypto fixture was 11/11 valid and 0/11 clean, and no complete report was emitted. The first guard-resume attempt ran from the candidate worktree without the benchmark submodule and failed before samples; the resumed batch ran from the root checkout. The failed Crypto batch is retained in the checkpoint, while `crypto.js-batch-2.json` is the successful seven-round top-up.

Percentages are candidate vs trunk. Wall, cycles, and instructions compare paired marginal-per-run values (`work − setup-only`); max RSS compares paired work-sample peaks. Intervals are deterministic 95% bootstrap intervals over the paired percentages, matching the fixed-work guard's bootstrap policy. Each row uses 12 clean pairs except RegExp, which uses 23. Negative means lower.

| Fixture | Wall | Cycles | Instructions | Work max RSS |
| -- | --: | --: | --: | --: |
| Crypto | +0.586% [−1.019, +2.456] | +0.687% [−0.796, +2.528] | −0.633% | −1.241% |
| DeltaBlue | −7.275% [−7.465, −6.866] | −7.272% [−7.437, −6.782] | −8.126% | −3.199% |
| EarleyBoyer | −1.183% [−1.449, −1.026] | −1.259% [−1.769, −0.943] | −2.105% | −9.831% |
| NavierStokes | −0.873% [−1.578, −0.541] | −0.781% [−1.772, −0.589] | −0.608% | −1.007% |
| RayTrace | −4.012% [−4.395, −3.764] | −4.034% [−4.474, −3.773] | −4.030% | −4.050% |
| RegExp | +1.356% [+0.583, +2.740] | +1.420% [+0.778, +2.668] | +0.763% | −2.975% |
| Richards | −1.158% [−1.467, −0.499] | −1.052% [−1.620, −0.576] | −0.654% | −3.125% |
| Splay | −6.803% [−7.340, −6.133] | −6.845% [−7.031, −6.077] | −4.645% | −24.608% |

The 12-pair all-eight result has no cross-fixture wall median above TOD-11's 2% WP6 cap, and every fixture's work max RSS fell. Splay's active marginal wall interval is below zero and its RSS fell 24.608%. RegExp is the only positive wall median (+1.356%); after 23 clean pairs its 95% interval still extends above 2%, while the same-binary A/A wall interval is −1.391% to +4.977%. Thus the candidate meets the point-estimate form of the written wall/RSS rule, but a conservative upper-confidence interpretation of the 2% ceiling remains unresolved. The generic fixed-work guard's `overall: rejected` reflects a different, cycle-based zero-regression rule; keep both outcomes distinct until TOD-11 aligns the tool and written policy. A stock candidate Splay diagnostic is now available below; it does not change the official distance or trunk row.

The stock engine preflight is `stock-preflight.json` (no timing). It verified the candidate interpreter binary, QuickJS 2026-06-04 SHA `4b0d0da1beae0a88d0724c7a0869797dee7a24c5bc1e74cfb5d2f69d17811c2b`, Bun 1.4.2 SHA `b36d2e9d9bd50e3a3b6a8f2d481839417d6600d4c1753411b094ee0da3222cef` (`BUN_JSC_useJIT=0`, verified by JSC option dump), and Node v26.10.0 SHA `4ca2a42d6d55a834b04d0892c5bc646a6869fcebec57f91c849dac0aa1031610` (`--jitless`, verified by `process.execArgv`).

The first candidate stock diagnostic is `stock-splay-11.json` (SHA-256 `47214135c063ea2d239c6f1d8a29a353fe149b82db1a1547de648958f5d89675`). All 11 rounds were valid and output-equal, but the campaign is retained as contaminated: host snapshots captured an external Deno worker at 174.6% and 154.2% CPU during round 5 (Node and Quench samples) and round 6 (Bun sample). Its observed medians were Quench 9,715 Score / 88,014,848 B max RSS, Node `--jitless` 14,880 Score, Bun no-JIT 9,966 Score / 63,635,456 B max RSS, and QuickJS 8,654 Score.

A clean Splay-only stock retake is `stock-splay-11-retry-1.json` (SHA-256 `5245f0ffe380be99d32d80eb6cac9f2bc2613cd419e1932997ed5e1afa160304`). All 11 rounds were valid and output-equal. Recorded one-minute load before/after samples ranged 2.68–3.54; no external Deno, Node, Bun, Cargo, or esbuild workers appeared in host snapshots (the largest unrelated listed process was `doctl` at 5.3% CPU). Medians: Quench 9,781 Score / 87,998,464 B max RSS; Node `--jitless` 14,913 Score / 407,355,392 B max RSS; Bun no-JIT 9,966 Score / 63,619,072 B max RSS; QuickJS 8,646 Score / 151,060,480 B max RSS. Node is the stock Score bar and Bun the RSS bar, so candidate diagnostic distances are 1.525× Score and 1.383× max RSS. The Splay-only result does not meet either bar. `qualification_ready=false` is the runner's full-eight-fixture flag; this report is scoped to the current one-benchmark task, and no trunk or official distance changes because the candidate remains isolated. Runner SHA is `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`, corpus commit `64e1860c736c1b899708f4cd646721bc71e53d8b`, and candidate executable SHA `afde285b7f8dff56da12f44eb915034b0599d7aec95a2ff39d55ae18a8d7c4cf`.

All fixed-work reports were launched from the root checkout to use its pinned corpus; the report's `source_revision`/`source_dirty` therefore describe that launcher checkout, not the engine source. The candidate executable identity is the pinned SHA above; its source is `11efad0ec7d7a4bf44885a07cc321627e79f017f`.
