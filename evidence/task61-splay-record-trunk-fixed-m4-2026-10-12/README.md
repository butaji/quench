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

## Candidate fixed-work screen (M4, preliminary)

Candidate binary SHA `afde285b…a8d7c4cf` vs trunk binary SHA `8432da11…bf5795dd`, runner SHA `bdc9d6c4…98db109`, corpus commit `64e1860c736c1b899708f4cd646721bc71e53d8b`. The resumable schema-5 guard has 7 clean paired rounds per fixture (one additional valid one-round pilot had incomplete contention calibration and is excluded from clean-pair wall/cycle summaries). All top-up reports are stored in `all-eight-ab-guard-topups/`; the guard checkpoint and raw decision are `all-eight-ab-guard.json` and `all-eight-ab-guard.decision.json`. The initial one-round pilot is `all-eight-ab-schema5-initial-1.json`.

Percentages below are candidate vs trunk, medians of the 7 detector-clean paired marginal-per-run samples for wall, cycles, and instructions, and paired work max RSS. Negative means lower. The 95% intervals are bootstrap intervals over the seven paired deltas.

| Fixture | Wall | Cycles | Instructions | Work max RSS |
| -- | --: | --: | --: | --: |
| Crypto | −0.358% [−1.788, +1.756] | +0.102% [−1.625, +0.877] | −0.632% | −0.977% |
| DeltaBlue | −7.213% [−7.553, −6.844] | −7.258% [−7.487, −6.808] | −8.125% | −3.318% |
| EarleyBoyer | −1.263% [−1.589, −0.984] | −1.187% [−1.567, −0.944] | −2.104% | −9.845% |
| NavierStokes | −0.874% [−2.005, −0.647] | −0.965% [−1.898, −0.629] | −0.608% | −1.010% |
| RayTrace | −4.012% [−4.624, −3.734] | −4.040% [−4.673, −3.822] | −4.026% | −4.219% |
| RegExp | +1.267% [+0.583, +4.318] | +1.420% [+0.778, +4.345] | +0.763% | −3.276% |
| Richards | −0.951% [−1.323, +0.189] | −1.038% [−1.338, +0.055] | −0.653% | −3.130% |
| Splay | −6.953% [−7.451, −6.327] | −6.858% [−7.055, −6.259] | −4.649% | −24.627% |

The fixed-work guard's `overall` field says `rejected` because it treats a positive cycle interval on RegExp as a regression. TOD-11's WP6 merge rule instead uses the other-fixture wall limit (2%) and RSS limit (1% beyond the A/A band); RegExp's seven-pair wall median is +1.267% and its RSS fell 3.276%. Its wall interval extends above 2%, so this seven-pair screen does not settle the written wall rule. The same-binary RegExp A/A wall interval is wide (−1.391% to +4.977%). Do not merge or claim an official Splay distance from this screen; complete 11 clean pairs and report both the tool result and the explicit TOD-11 wall/RSS evaluation. The official stock distances remain 1.664× Score / 1.810× max RSS.

An 11-round confirmation was interrupted after its Crypto fixture because six Deno workers raised one-minute load to 16.56 on the 10-CPU M4; Crypto was 11/11 valid and 0/11 detector-clean. The process was stopped before using those samples, and no complete report was produced. The first guard-resume attempt also ran from the candidate worktree, where the benchmark submodule is absent; it exited before collecting samples. Resuming from the root checkout succeeded. This is recorded in the checkpoint; the successful Crypto top-up is `crypto.js-batch-2.json`.
