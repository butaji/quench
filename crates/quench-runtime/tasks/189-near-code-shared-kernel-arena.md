# 189 — Near-code shared-kernel arena

Status: planned

Allocate rustc-cooked immutable kernels and linked stencil images from one reserved code
arena so AArch64 direct branches remain in range. Each kernel is instantiated once per
arena, not once per function. If a target cannot fit, emit a small shared island/veneer;
never silently fall back to a far indirect call without recording it in link statistics.

Kernel placement is immutable `KernelPlacement` data consumed by the linker. It is
orthogonal to Task 180: huge pages improve iTLB reach, while this task changes indirect
or badly predicted cross-region calls into nearby direct branches.

All branch ranges, arena sizes, alignment, and island budgets are named architecture
constants with checked arithmetic. Acceptance: map inspection and disassembly prove
direct `b`/`bl` reachability; one kernel address is shared across functions; fallback
islands are correct; native branch/call evidence and alternating V8v7 A/B justify keeping
the arena on Apple Silicon.

Primary source: V8 observed far JIT-to-builtin calls being consistently mispredicted on
Apple M1 and recovered performance by moving/copying code closer:
<https://v8.dev/blog/short-builtin-calls>.

## Round-nineteen refinement: PC-relative code/data closure

Run an isolated arena experiment before larger compiler work. Place each linked image,
its relocation-closed local data from Task 274, shared kernel entry islands, and branch
veneers inside a named architecture reach window. Prefer PC-relative internal code/data
relocations; patch an absolute address only for a target that cannot legally inhabit the
arena, and count every such escape. `AARCH64_DIRECT_BRANCH_REACH_BYTES`,
`AARCH64_LITERAL_REACH_BYTES`, `CODE_ARENA_ALIGNMENT_BYTES`, and
`MAX_KERNEL_ISLAND_BYTES` must be named checked constants rather than embedded offsets.

A 2025 copy-and-patch implementation for R reports that its PC-relative memory model
improved compilable-workload runtime by more than 8% on average, reduced executable size
by 26%, and improved simple workloads more strongly. Those results make locality a
high-priority small experiment here, not a transferable performance promise:
<https://d3s.mff.cuni.cz/publications/kocourek_copyandpatch_2025/>.

## First isolated implementation experiment

Run this only after Task 148's object-handle slice has a frozen accepted baseline. Keep
Task 274's `-Os`/`-Oz` local-data atoms out of the first comparison so the experiment has
one cause.

Replace the current ownership-only `CodeArena` with one bounded macOS AArch64 reservation.
Reserve `CODE_ARENA_RESERVATION_BYTES` as inaccessible pages, commit each allocation as
read/write, copy and patch it, invalidate the instruction cache, and finally protect the
whole page-aligned allocation read/execute. Never place two independently published
images in one protection page. Place one arena-local slab containing the exit kernel and
effect-reentry kernel before allocating function images; each kernel identity occurs once
per arena.

Change only the two function-to-kernel adapters in the first experiment. Represent their
target as an unresolved typed kernel-edge hole until the function's arena address is
known, then encode a checked AArch64 unconditional `b` to the arena-local kernel. The
existing internal/symbolic bytecode branches remain unchanged, as do absolute helper,
site-table, and metadata pointers. An out-of-range edge must increment a named counter and
use the existing literal-load/indirect-branch adapter; it must not fail silently. A veneer
is a later refinement if the bounded reservation proves insufficient.

Use named constants for the signed AArch64 branch reach, reservation size, page and image
alignment, kernel-slab size, and maximum padding. Checked arithmetic must prove that the
configured arena window keeps every in-arena kernel edge representable.

Record `arena_reserved_bytes`, committed bytes, alignment waste, images, kernel identities,
direct kernel edges, out-of-range edges, and fallback mappings. Tests must inspect cooked
words to prove the direct edge is `b`, verify both displacement boundaries, verify one
kernel address across multiple functions, and exercise the fallback adapter. The isolated
gate is release tests, complete smoke, code/map statistics, disassembly, and an alternating
full V8v7 A/B against the frozen post-Task-148 binary. Reject the direct-edge rewrite if it
does not improve the aggregate; the arena allocation API may be retained only if it
independently reduces mappings or committed memory without a runtime regression.
