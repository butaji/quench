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
