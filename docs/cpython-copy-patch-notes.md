# CPython copy-and-patch comparison

This is a source-based comparison, not an assumption that Quench can copy
CPython's C ABI. The relevant CPython implementation is in the main branch:

- [InternalDocs/jit.md](https://github.com/python/cpython/blob/main/InternalDocs/jit.md)
  describes the trace recorder, executor entry, invalidation, and stencil
  generation pipeline.
- [`Python/jit.c`](https://github.com/python/cpython/blob/main/Python/jit.c)
  implements the executable allocation, relocation patching, AArch64
  trampolines, W^X transition, and executor lifetime.
- [`Tools/jit/_targets.py`](https://github.com/python/cpython/blob/main/Tools/jit/_targets.py)
  selects target flags and extracts relocations/DWARF data. AArch64 uses PIC,
  disables outlined atomics, and emits frame-pointer/unwind metadata.
- [`Tools/jit/README.md`](https://github.com/python/cpython/blob/main/Tools/jit/README.md)
  records the LLVM build requirements; LLVM is used at build time, including
  `llvm-objdump`, `llvm-readobj`, and `llvm-dwarfdump`.
- [AArch64 trailing-jump PR #131042](https://github.com/python/cpython/pull/131042)
  reports a 1.4% Linux improvement from removing a jump at the end of every
  micro-op stencil while preserving alignment with NOPs.
- [JIT code-quality issue #115802](https://github.com/python/cpython/issues/115802)
  discusses position-independent calls, `preserve_none`, and splitting hot
  code from cold error/deoptimization stubs.
- The underlying [Copy-and-Patch paper](https://arxiv.org/abs/2011.13127)
  explains why precompiled templates are useful only when enough work is
  composed to amortize entry and exit overhead.

## What CPython actually does

1. The adaptive interpreter records a hot trace and lowers it to Tier-2 uops.
   The start bytecode is replaced by `ENTER_EXECUTOR`.
2. `_PyJIT_Compile` computes the complete trace's code/data/GOT/trampoline
   sizes, allocates one page-rounded mapping (bounded below 1 MiB), emits all
   stencils, patches relocations, clears the instruction cache, and changes
   the mapping to read/execute.
3. The executor has one CPS-style entry (`jit_func`) receiving the frame,
   stack, thread state, and top-of-stack caches. It returns the next Tier-1
   instruction pointer. Intermediate uops do not return through the VM loop.
4. Dependencies are tracked in contiguous executor arrays and invalidation
   drops the executor and restores the original bytecode. This makes a cached
   native pointer safe only while its owning executor remains live.
5. On AArch64, direct relative calls/branches are used when in range; otherwise
   CPython emits a 16-byte `ldr x8, literal; br x8` trampoline. It also flushes
   the I-cache before enabling RX permissions.

## Implications for Quench

The current typed entry cache fixes the repeated Quench lifecycle/address work,
and is validated by the synthetic numeric-loop win. Broad `NativeRegionPlan`
rows still enter an AArch64 trampoline whose target is a Rust bridge that loops
over canonical handlers. A narrow exception is now real machine composition:
the generated two-`Add` numeric chain executes `FADD; FADD; RET` through one
typed entry, while its producer/consumer and number guards remain in Rust.
The host-derived `ExecutionPolicy` now keeps this capability distinction in one
place: ARM scalar leaves may be explicitly enabled, while the bridge and the
fused-region/optimizing views remain gated until a real composed region is
available.

The next performance experiment should therefore widen the proven composed ARM
region set, measured before and after with DWARF and retired instructions/cycles:

- one cached entry per hot region, with arguments/state in a stable ABI;
- fallthrough between ordinary operations and one return only at the region
  boundary (no `ret` per operation);
- explicit cold/deoptimization exits so the hot path stays linear;
- direct AArch64 relocations/trampolines and I-cache maintenance;
- a bounded, disposable mapping with dependency invalidation and complete
  fallback on every semantic or physical miss.

CPython's `preserve_none` discussion is a hypothesis, not a free win: its issue
reports an estimated 0--6% improvement with no AArch64 gain, while hot/cold
splitting was estimated at 1--2%. Quench should test register spills at the
actual ARM boundary rather than add an ABI shim speculatively. Likewise,
removing trailing branches is only meaningful after Quench has multi-operation
regions to chain; the existing 8-byte ARM leaves already end in `ret` and have
no internal trailing jump to remove.
