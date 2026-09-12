# 367 — Differential rustc/LLVM stencil-cooker audit

Status: in_progress

Make compiler-produced patch-hole extraction fail closed across rustc/LLVM versions. For
every cooked template and every typed placeholder family, produce three controlled builds:

1. A and B use identical named placeholder values; bytes, symbols and relocation manifests
   must be identical.
2. C uses different valid named values.
3. The byte/bit difference `A xor C` must be contained exactly by declared typed-hole masks;
   every expected hole must change and no undeclared byte may change.

Generate minimum, maximum and one-out-of-range cases for every immediate kind. An
out-of-range operand selects a named fallback template or relocation-closed data atom and
can never truncate. Validate split instruction fields, duplicate sentinels, instruction
selection changes, alignment/padding, hot/cold sections, local data, branch/call targets and
the terminal connector transfer. Store the compiler flags and object hashes in the audit
manifest. No numeric instruction mask or limit may appear outside its named encoder.

Integrate this audit with Tasks 102, 316 and 347 in `build.rs`/CI. Runtime linking remains
copy plus typed patch only; the verifier adds no executed overhead. Acceptance requires
negative fixtures for missing, duplicate, wrong-kind, partially changing and undeclared
holes, plus a clean audit of the complete catalog on the pinned and current toolchains.

Primary source: Ertl and Paysan, “Code Copying in Gforth: 20 Years' Experience”
<https://www.complang.tuwien.ac.at/papers/ertl%26paysan25kps.pdf>.

## Implemented slice

`DEEGEN_STENCIL_COOKER_AUDIT=1` now makes `build.rs` compile production A, identical B,
and changed-placeholder C catalogs. It compares every handler by symbol, requires A and B
bytes plus typed manifests to be identical, and proves each A/C instruction XOR is wholly
contained in the declared AArch64 immediate-field mask. Every declared operand/site hole
must change, symbol sets and code sizes must remain equal, connector relocation validation
still runs for A and B, and the build emits toolchain, flags, counts, sizes and object
fingerprints in `stencil_cooker_audit.txt`.

The changed values come from named, non-overlapping, still-encodable deltas in
`stencil-aot/operand_holes.rs` and `stencil-aot/site_holes.rs`; they do not exist in the
runtime catalog. `scripts/audit-stencil-cooker.sh` runs the complete audit for the two
closed-template optimization levels accepted by Task 273. Runtime patcher tests now cover
the maximum encodable operand/site value, first out-of-range value, and an unaligned
operand byte offset.

The first O2 run passed over 144 symbols, 47 changed stencils and 89 typed holes. Remaining
before completion: committed O2/O3 manifests, negative extractor fixtures for missing,
duplicate, wrong-kind, partial and undeclared holes, plus the current-toolchain lane when it
differs from the pinned compiler.

After Task 373 added three cooked primitive handlers, the complete audit was rerun at O2
and O3 in `reports/task373-stencil-cooker-audit/`. Both pass over 147 symbols, 47
placeholder-sensitive stencils, and all 89 typed holes. The unchanged hole count is expected:
the new boolean/property handlers consume runtime site/IC fields and add connector
relocations, not typed operand placeholders.

After Task 375 added four macro-generated equality handlers, both O2 and O3 audits pass in
`reports/task375-stencil-cooker-audit/` over 151 catalog symbols, 47
placeholder-sensitive stencils, and all 89 typed holes. The unchanged typed-hole count is
again expected: equality operands use existing site fields and connector relocations.

After Task 378 added the direct lexical-address load handler and extended the shared guest
frame ABI, both optimization levels pass in `reports/task378-stencil-cooker-audit/` over
152 catalog symbols. The current optimized catalog exposes 46 placeholder-sensitive
stencils and 88 typed holes at both levels; A and B object fingerprints are identical and
C differs only inside declared masks. The direct name handler consumes POD frame/site
fields and adds no new operand placeholder.

After Task 381 added two direct dense-computed leaves and a shared object-layout
projection, both optimization levels pass in `reports/task381-stencil-cooker-audit/` over
154 catalog symbols. O2 and O3 each report 46 placeholder-sensitive stencils and 88 typed
holes; A/B fingerprints are identical and variant C changes only declared masks.

After Task 368 generalized cooked static-property reads to the bounded inherited projection,
both optimization levels pass in `reports/task368-stencil-cooker-audit/` over 167 catalog
symbols, 51 placeholder-sensitive stencils, and 110 typed holes. A/B fingerprints are
identical and variant C differs only within declared masks.
