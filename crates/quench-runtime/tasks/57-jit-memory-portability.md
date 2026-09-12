# 57 — Runtime page-size and JIT-memory-permission portability

Status: planned

Two concrete, currently-latent portability bugs in `map_executable`/`a64_abi` (`main.rs`), found while defining the architecture target matrix ([[55-architecture-target-matrix]]):

## 1. Hardcoded 4 KB page size

`a64_abi::PAGE_BYTES = 4096` is a compile-time constant, but page size is **not uniformly 4 KB across this project's own target matrix**: Apple Silicon macOS uses 16 KB pages; Linux/AArch64 kernel configurations vary (4 KB, 16 KB, and 64 KB pages all exist in the wild, e.g. some server distributions and Android-derived kernels default to non-4 KB pages). Any arithmetic in the executable-memory allocator that assumes 4 KB granularity (region packing, allocation rounding) is silently wrong on any target where the real page size differs — `mmap`/`mprotect` themselves round to the true kernel page size regardless of what the caller assumes, so a mismatch doesn't necessarily crash, but it can waste memory (under-using a 16 KB page as if it were 4 KB) or, more seriously, cause incorrect size/offset arithmetic anywhere `PAGE_BYTES` is used to compute a boundary rather than just requested as a minimum.

**Fix**: query the real page size at process startup via `sysconf(_SC_PAGESIZE)` (POSIX, available on both macOS and Linux) once, cache it, and use that value everywhere `PAGE_BYTES` is currently referenced, rather than a compile-time constant.

## 2. `MAP_JIT`/`pthread_jit_write_protect_np` gap on macOS Hardened Runtime

`map_executable`'s macOS path calls plain `mmap`+`mprotect` with no `MAP_JIT` flag and no `pthread_jit_write_protect_np` toggling. This works today because the CLI binary runs unsigned/ad-hoc in local development. It will **fail under Apple's Hardened Runtime** (required for notarization/distribution): a hardened, codesigned binary without the `com.apple.security.cs.allow-jit` entitlement cannot `mprotect` a page to be simultaneously (or successively) writable and executable at all; even with the entitlement, Apple's documented pattern requires allocating with `MAP_JIT` and wrapping every write-to-executable-memory step in `pthread_jit_write_protect_np(0)`/`pthread_jit_write_protect_np(1)` to toggle the actual W^X state per-thread.

**Fix**: gate an alternate macOS allocation path behind whether the binary is expected to run under Hardened Runtime (a build-time or documented deployment choice, not detected at runtime) — add `MAP_JIT` to the `mmap` flags and wrap the `ptr::copy_nonoverlapping` write (and any later [[48-submorphism-repatch-on-guard-miss]]-style in-place patch) in `pthread_jit_write_protect_np` calls. Document which deployment mode (ad-hoc local dev vs. notarized distribution) each path is for, since the two are mutually incompatible entitlement postures, not a strict upgrade of one over the other.

Acceptance: page-size-dependent arithmetic uses a runtime-queried value verified correct on at least one 16 KB-page target (Apple Silicon macOS) and one 4 KB-page target (standard Linux/x86-64 or Linux/AArch64); a documented, tested path exists for running under macOS Hardened Runtime with the JIT entitlement, exercised by at least a manual verification step since CI is unlikely to run under a notarized/hardened build; both fixes are covered by [[05-performance-harness]] running unmodified on every existing target.

Note: [[59-colima-docker-linux-environment]]'s spike confirmed standard Docker/Colima Linux containers (both native aarch64 and QEMU-emulated x86-64) report 4 KB pages — that environment validates the common 4 KB case but does **not** exercise the 16 KB/64 KB Linux/AArch64 configurations this task is also concerned about; a real non-4-KB-page host or a unit-level page-size-injection test is still needed for that specific case.
