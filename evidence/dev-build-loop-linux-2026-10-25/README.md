# Development edit-and-test loop: cargo, ra-jit and the Cranelift codegen backend

Question: can a faster build tool shorten a lane's edit → test loop on `quench-runtime`?

**Host:** 4-vCPU x86_64 cloud VM, 15 GiB RAM.
**Source:** v2-merged `d2327ad5e`, in a separate worktree with its own target directory per mode.
**Loop:** `buildbench.py MODE` does one untimed full build, then three rounds of: change
`Vm::plain_data_equal`'s body (one `black_box` line), and run
`cargo test -p quench-runtime --lib -- --exact vm::tests::plain_data_equality_proves_only_plain_data`.
It records wall time, the largest child's peak RSS, the ra-jit daemon's peak RSS, and the
test outcome.

| Mode | Toolchain | Full build | Edit → test (3 rounds) | Peak memory |
| -- | -- | --: | -- | -- |
| cargo, incremental (repo `profile.dev`) | 1.99.0 | 122 s | 41.4 / 41.0 / 40.7 s | 5.3 GB rustc |
| cargo, `CARGO_INCREMENTAL=0` (gate style) | 1.99.0 | 112 s | 69.9 / 68.5 / 70.8 s | 5.2 GB rustc |
| ra-jit 0.1.0 (`cargo ra-jit test`) | 1.99.0 + rust-src | 136 s | 59.0 / 149.3 / 50.0 s | 5.9 GB rustc + 12.4 GB daemon |
| rustc Cranelift backend (`-Zcodegen-backend`, `RUSTFLAGS="-D warnings"`) | nightly 2026-10-24 | 111 s | 22.3 / 21.7 / 22.3 s | 3.2 GB |

**ra-jit:** slower than incremental cargo on this crate, and its daemon peaked at 12.4 GB of a
15 GB host. Its log lists many functions it cannot compile ("writable static … without a rustc
symbol": the env-switch `OnceLock`/atomic statics in compiled.rs, wasm_js.rs, heap, compile), so
edits touching those modules fall back to rustc. A test-file change also went to cargo (a
build-script output of quench-regexp changed). Version 0.1.0 has no borrow checker.

**Cranelift backend:** half of incremental cargo's edit latency (22 s vs 41 s) and 40% less memory.
To run, it needs:
- `.cargo/config.toml`'s LLVM-only `-C llvm-args=-align-all-functions=6` replaced (via
  `RUSTFLAGS`);
- a portable form of `value::truncate_to_i32`: the backend does not implement
  `llvm.x86.sse2.cvttsd2si` (rustc_codegen_cranelift#171), so the unpatched test binary aborts
  (`cranelift-unpatched.log`). With truncation, or `i32::MIN` when out of range or NaN, the
  result is the same as the instruction.

With that patch, runtime lib 969/970 and regexp 20/20 pass; the full runtime suite ran in 78 s.
The one failure, `subset_tests::native_stack_scopes_are_ordered_truthful_and_unwind_safe`,
needs `catch_unwind` after a panic, which the backend does not support (panics abort). The
inline `asm!` and the regexp SSE2 intrinsics work.

**Not measured:** release, PGO or BOLT builds (every benchmark and qualification build stays on
LLVM 1.99.0), the gate's full multi-crate build, and aarch64.
