# 378 — Direct lexical-address load/store stencils

Status: in_progress

Lower statically resolved outer/global name operations to a canonical lexical address
instead of making the hot path rediscover a source string:

```text
LexicalAddress = { depth, slot, expected_layout }
```

OXC binding information and the existing immutable function layouts produce the address.
The bytecode/site record owns that one fact; interpreter-like semantics, stencil
selection, diagnostics, and slow paths derive their views from it. A rustc/LLVM-cooked
load or store leaf burns the named environment-chain and value-slot offsets, guards the
expected layout identity when mutation can change the mapping, and accesses the fixed
slot directly. A failed guard enters the canonical unexecuted-op slow kernel, which may
resolve and republish a new address.

Do not duplicate captured values merely to flatten lookup. Mutable captured bindings must
remain shared cells/slots, closures must observe later writes, and ownership transfer on
stores must use the existing canonical overwrite effect. Dynamic scope, `eval`, `with`,
or an unbounded lexical depth select the general kernel. The supported MVP subset should
normally have no such dynamic binding construct.

Both leaves are ordinary `Stencil<Connector, Connector>` morphisms. A load changes the
register-value component of context; a store changes the environment effect token. The
lexical address is an external patch obligation, so composition remains associative and
the same leaf can participate in opcode, block, loop, and function-level expressions.

Measured motivation: Task 373 saw `LoadName` as the first unsupported operation for
9,150,871 residual helper entries and `StoreName` for 489,045. Name-only capability was
estimated to close 438,003 entries; name plus strict equality 1,597,120; name plus call
5,502,818. These counts prioritize the family but never shape its semantics.

Acceptance: first-access and warmed-access disassembly contain no string hash, name-map
walk, `Rc` clone, or Rust name helper; direct hit/miss counters prove use; mutable capture,
shadowing, recursion, layout change, closure lifetime, ownership, and exception tests pass;
residual whole-block cover increases; the exact randomized A/B gate passes before default
enablement.

Primary precedents: register/upvalue indexing in Lua 5.0
<https://www.lua.org/doc/jucs05.pdf>, direct register-frame lowering in Sparkplug
<https://v8.dev/blog/sparkplug>, and classic display-indexed lexical access as summarized
in the University of Iowa compiler notes
<https://homepage.cs.uiowa.edu/~jones/compiler/spring13/notes/29.shtml>.

## Accepted direct-load slice

Ordinary `LoadName` now selects the rustc/LLVM-cooked
`deegen_dyn_load_name_cached` stencil from first execution. A site owns one POD
`NameIc = { depth, slot, layout }`. Its empty layout pointer is the named invalid state.
The canonical slow kernel resolves the source name and publishes this record on the first
miss. Later hits index a bounded `EnvironmentAccess` chain, guard immutable layout
identity and slot bounds, and load the word directly without hashing a string, walking
`Rc<RefCell<Environment>>`, or calling a Rust helper.

`Environment` owns one canonical `EnvironmentAccess = { layout, values, len }` projection.
The call frame borrows pointers to those projections for its bounded lifetime. The shared
guest-frame macro defines the host and AOT layouts together and derives every offset from
named word constants. `NameIcSite`, `NameIc`, and `EnvironmentAccess` are `repr(C)` POD
records with compile-time size/alignment checks.

The first layout-change test found a real invalidation defect: `Rc::make_mut` can mutate a
uniquely owned name map without changing its address. Environment layouts are now truly
immutable shape objects. Adding a binding clones the map and installs a new `Rc`, so the
layout pointer is a valid identity guard. Existing-value stores preserve the layout.

Raw word copying is restricted to values whose ownership representation permits it.
Numbers, booleans, null, undefined, and traced object handles stay native. `Rc`-backed
strings, functions, and regular expressions enter the canonical slow kernel so reference
counts remain correct. This is a current coverage boundary, not an alternative semantic
implementation.

All 143 release tests pass, including first fill, warmed hit, layout invalidation/refill,
captured-value mutation, direct block selection, dense snapshot separation, and
host/AOT ABI agreement. The O2 and O3 differential cooker audit passes in
`reports/task378-stencil-cooker-audit/` over 152 symbols and 88 typed holes.

## Rejected eager-snapshot designs

Three measured implementations copied ordinary name values into per-activation snapshots.
All were removed:

- per-PC eager refresh: aggregate -5.84%, including Richards -21.98%, DeltaBlue -9.54%,
  and Earley-Boyer -11.50% in `reports/task378-direct-name-ab-5/`;
- name-deduplicated eager refresh: aggregate -5.22%, including Richards -22.34% and
  Earley-Boyer -12.66% in `reports/task378-direct-name-dedup-ab-3/`;
- name-deduplicated refresh through an existing lexical IC: aggregate -0.23%, including
  Earley-Boyer -5.19% in `reports/task378-direct-name-cached-refresh-ab-5/`.

The ownership guard was added after the first snapshot prototype exposed incorrect raw
copies of `Rc`-backed values. The deeper performance failure was architectural: entry-time
refresh charges every activation, including executions that never reach a given load.
The accepted IC is lazy data at the use site.

## Performance evidence

The accepted binary is `/tmp/deegen-task378-lazy-ic-candidate`, SHA-256
`a34c215b5e0e8e799e5cfb79e70fb942a2aed7c7aea52ea2696836e344455f6a`.
Against Task 375's preserved binary, the three-pair development screen improved 1.47%.
The authoritative nine-pair exact comparison in
`reports/task378-direct-name-lazy-ic-exact-ab-9/` passed:

- aggregate 2293.53 -> 2326.86, +1.45%, 95% paired bootstrap interval
  `[+0.67%, +2.13%]`;
- Richards +4.65% `[+3.48%, +5.70%]`;
- DeltaBlue +6.47% `[+4.39%, +8.91%]`;
- Crypto +2.47% `[+1.49%, +3.36%]`.

The candidate increases composed code from 667,300 to 719,828 bytes, direct blocks from
1,901 to 2,030, direct opcodes from 6,225 to 6,811, and native entries from 140,237,279 to
142,242,550 in the first exact records. This is now a real physical-cost sample for Task
377: 52,528 copied bytes bought 129 more direct blocks, 586 direct opcodes, and roughly
two million more native entries in that process.

Task 378 remains in progress because `StoreName` still uses the canonical effect kernel,
and the direct load still sends reference-counted source/destination values to slow code.
Finish only after the store morphism and ownership-safe load/store arms pass their own
correctness and exact A/B gates.
