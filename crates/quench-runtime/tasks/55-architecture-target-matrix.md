# 55 — Architecture target matrix and stencil-ABI abstraction boundary

Status: planned

Today the entire native-stencil path (`a64_abi` in `main.rs`, all `deegen_*` handlers in `stencil-aot/handlers.rs`, `build.rs`'s `AARCH64_TAIL_BRANCH_OPCODE`/`AARCH64_BRANCH_OPCODE_MASK`) is AArch64-only, gated `#[cfg(target_arch = "aarch64")]`; anything else falls through to the diagnostic AST interpreter. Define the actual supported deployment matrix explicitly and the abstraction boundary needed to add a second architecture without duplicating category/composition logic.

## Target matrix

| | macOS | Linux |
|---|---|---|
| **AArch64** | Primary dev target today (`a64_abi`, `sys_icache_invalidate`). Apple Silicon: 16 KB pages, may require `MAP_JIT`/`pthread_jit_write_protect_np` under Hardened Runtime (see [[57-jit-memory-portability]]). | Supported today via the same `a64_abi` module; `__clear_cache` for icache flush; page size is **not guaranteed 4 KB** on all Linux/aarch64 kernel configs (some server distros ship 16 KB/64 KB pages) — see [[57-jit-memory-portability]]. |
| **x86-64** | Not targeted (Apple has discontinued Intel Mac hardware as a forward target; do not build this combination). | **Not yet implemented** — falls through to the AST interpreter today. See [[56-x86-64-sysv-backend]]. |

Two target combinations matter going forward: **macOS/AArch64** (primary dev, Apple Silicon) and **Linux/x86-64** (deployment/CI, the combination most server/CLI JS-runtime users actually run). Linux/AArch64 is a useful secondary (already mostly working, needs the page-size fix in [[57-jit-memory-portability]]).

## Abstraction boundary

The categorical structure (`Stencil<In, Out>`, `Kernel`, composition, connector states in `main.rs`) is already architecture-agnostic — it operates over `Vec<u8>`/`Rc<[u8]>` byte buffers, relocation `Hole`s, and `CopyPatch` operations, none of which mention AArch64 specifically. The architecture-specific surface is narrowly:

1. **Instruction encoding constants** (`a64_abi`'s `SAVE_CONNECTORS`, `BR_BASE`, `FADD_D0_D0_D1`, etc.) — a fixed table of opcodes/bit-layouts per architecture.
2. **Register/calling-convention assignment** (`ACC = 19`, `FRAME = 20`, `FP_ACC = 0` in `a64_abi`) — which physical register holds which piece of persistent interpreter state.
3. **Hole/relocation patching arithmetic** — AArch64 encodes immediates as bit-fields packed into specific positions within a 32-bit instruction word (`BR_IMM_MASK = 0x03ffffff`, `LDR_D_LITERAL_IMM_MASK`); this arithmetic is architecture-specific and must not leak into `Hole`/`CopyPatch`'s architecture-neutral representation in `main.rs`.
4. **Memory mapping and cache coherence** (`mmap`/`mprotect`/`sys_icache_invalidate`/`__clear_cache`) — already OS-gated correctly; needs arch-gating added alongside the existing OS-gating once a second architecture exists.
5. **The `build.rs` tail-branch recognition constant** (`AARCH64_TAIL_BRANCH_OPCODE`) — architecture-specific bit pattern identifying the patchable tail-transfer instruction in extracted AOT template bytes.

Concrete step: extract items 1–3 into a per-architecture module implementing one shared trait/const-table shape (e.g. `trait StencilAbi { const ACC: u32; const BR_BASE: u32; fn encode_branch(...) -> u32; ... }` or an equivalent const-generic module pattern already established by `a64_abi`), so [[56-x86-64-sysv-backend]] adds `x64_abi` as a sibling module rather than threading `#[cfg(target_arch = ...)]` through composition-level code.

Acceptance: `a64_abi`'s contents are fully reachable through the new shared boundary with no behavior change (regression-tested via [[05-performance-harness]]); no architecture-specific constant or bit-encoding logic exists outside a per-architecture module; `build.rs`'s tail-branch recognition is parameterized by architecture rather than hardcoded to the AArch64 constant.
