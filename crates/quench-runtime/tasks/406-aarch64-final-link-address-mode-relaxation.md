# 406 — AArch64 final-link address-mode relaxation

Status: planned

Extend the general patch algebra so an address obligation describes a value and legal
equal-width realizations instead of committing the stencil template to one instruction
sequence before placement. After Task 348 assigns final addresses, select the cheapest
representable form:

```text
AddressMode = DirectBranch26
            | PageRelativeAdrpAdd
            | LiteralLoad
            | MovWideImmediate
            | IndirectAbsolute
```

The choice depends only on checked address distance, target kind, architecture features, and
the immutable code layout. It never depends on benchmark identity, source spelling, runtime
execution count, or hotness. All branch/page/literal ranges, instruction widths, lane counts,
and veneer limits are named target constants. Every realization must preserve the template's
fixed byte extent during the first experiment; changing layout belongs to Task 348's explicit
fixed-point pass.

Compose this with Task 189 rather than duplicating kernels: a `Kernel` has one immutable
arena-local address shared by every function, while each `StencilInstance` patches its own
shortest legal reference to that address. Out-of-range references use a shared veneer or the
existing absolute form and increment a diagnostic counter.

Acceptance: cooker metadata proves every alternative originated from a rustc/LLVM-produced
relocation shape; unit tests hit exact positive/negative range boundaries and fallback;
disassembly shows fewer indirect branches/address loads; code size does not regress; all
release and forced-GC tests pass; and the randomized nine-pair V8v7 gate is nonnegative with
no component-floor violation. A cross-language result is motivation only, never a projected
speedup.

Primary sources:

- CPython's current AArch64 copy-and-patch relaxation logic:
  <https://github.com/python/cpython/blob/main/Python/jit.c>
- LLVM JITLink fixup/relaxation model: <https://llvm.org/docs/JITLink.html>
- Copy-and-patch R PC-relative experiment:
  <https://www.itspy.cz/wp-content/uploads/2025/09/it_spy_2025_diplomova_prace_50.pdf>

