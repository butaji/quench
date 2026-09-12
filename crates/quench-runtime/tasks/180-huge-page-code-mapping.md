# 180 — Huge-page mapping for the linked executable image

Status: planned

`src/main.rs`'s three `mmap`/`mprotect` call sites (around lines 2463-2576, 2735-2750,
and 3128-3156) map every linked stencil image through plain anonymous 4 KB (or 16 KB on
Apple Silicon, per [[57-jit-memory-portability]]) pages. As [[14-code-image-reuse]] and
[[42-whole-program-hash-consing]]/[[135-shape-hash-consing]] grow the total linked-code
footprint, and as [[80-persisted-linked-image-cache]] keeps more images resident across
runs, instruction-TLB pressure grows with it: a 4 KB page covers far less code than a
huge page, so a growing executable image means more iTLB misses and page-walk latency
on every far jump/call between stencils, independent of any per-stencil codegen quality
work in [[43]]/[[52]]/[[30]].

Standard huge pages (2 MB on x86-64/typical Linux configurations) reduce the number of
iTLB entries needed to cover a given code size by roughly 512x, directly cutting iTLB
misses and page-walk cost; this is a well-established technique for large statically- or
dynamically-generated code regions, not specific to any one JIT design. Map executable
regions with `MAP_HUGETLB` (or `madvise(..., MADV_HUGEPAGE)` for transparent huge pages)
where the underlying platform supports it, falling back to the existing plain mapping
where it does not — this is a mapping-flag change at the three existing `mmap` call
sites, not a new allocator or a change to stencil-composition semantics.

This is capacity-gated, not correctness-gated: below some total linked-code size a huge
page is pure waste (a partially-used 2 MB page costs more resident memory than several
4 KB pages), so apply it only once total mapped executable size crosses a measured
threshold, or batch multiple function images into one huge-page-backed region rather
than giving every small function its own huge page.

Acceptance: executable regions use huge-page-backed mappings on platforms/configurations
where available, verified by inspecting the resulting mapping (`/proc/self/smaps` on
Linux, or the platform equivalent); falls back correctly (no crash, no silent
misbehavior) on a platform/kernel without huge-page support; alternating A/B on the full
V8v7 suite after [[14]]/[[37]] have grown the linked-image corpus shows a measured gain
with no correctness regression; resident-memory overhead from partially-used huge pages
stays within a stated budget.

Primary sources:
- Intel, Runtime Performance Optimization Blueprint — Large Code Pages: <https://www.intel.com/content/dam/develop/external/us/en/documents/runtimeperformanceoptimizationblueprint-largecodepages-q1update.pdf>
- Linux kernel docs, Transparent Hugepage Support: <https://docs.kernel.org/admin-guide/mm/transhuge.html>
