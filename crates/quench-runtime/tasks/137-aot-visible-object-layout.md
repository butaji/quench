# 137 — AOT-visible object layout and property IC descriptors

Status: complete

Task 136 proved that erasing residual bytecode dispatch is insufficient when the
property hit still crosses an indirect Rust helper boundary. Define one stable,
`repr(C)` object/property ABI that rustc AOT stencils can read directly: a receiver
shape identity/version, fixed-slot base pointer, and a per-site monomorphic descriptor
containing expected shape plus slot. The descriptor is mutable data filled by the
canonical slow semantics; stencil templates and shared kernels remain immutable.

This representation must be the authoritative object state, not a mirrored fast header
that can drift from `PropertyStorage`. The Lisp/data-first constraint is one fact in one
place: ordinary Rust semantics and AOT code are two interpreters of the same layout.
The direct hit path must contain no `HashMap`/`IndexMap` lookup, no Rust helper call, and
no `RefCell` layout dependency. Heap-valued loads/stores must preserve ownership; if the
current `Rc` model prevents a direct operation, expose that as an explicit connector or
first complete the relevant Task 09 lifetime change instead of copying unsound words.

Start with fresh native profiles on the exact Task 133 binary and an ABI feasibility
audit. Then implement the smallest stable layout slice required by own static-property
ICs. Acceptance: compile-time offset/size assertions shared with the AOT crate; IC miss,
shape transition, prototype shadowing, aliasing, and heap ownership tests; disassembly
showing the hit path is loads/compare/branch/direct slot access with no helper call;
complete smoke; alternating full-suite A/B. Reject mirrored derived state or any
benchmark/property-name selector.

## Result: accepted

The authoritative layout is now explicit end-to-end:

- `ShapeRef` is the one-word canonical identity owned by the global structural intern
  table;
- `PropertySlots` is a `repr(C)` owning buffer whose pointer/length/capacity fields are
  the actual allocation state, not a mirror of `Vec`;
- `ObjectCell` places `UnsafeCell<Object>` at offset zero and keeps its checked borrow
  state after the value; and
- `PropertyIc` stores the canonical shape pointer plus slot, with the first two words of
  each property site exposed directly through `InlineSite::literal`.

Compile-time offset assertions nail the Rust/AOT contract. The first consumer is the
general six-bytecode grammar
`LoadLocal; GetStatic; LoadLocal; GetStatic; StrictEq; JumpIfFalse`. Its rustc-produced
AArch64 stencil directly guards two shapes, loads two slots, implements strict numeric
and immediate/pointer identity, and rejoins the canonical slow block for misses or
string-content equality. It neither materializes dead bytecode temporaries nor calls a
Rust helper. The cooked disassembly in the Task 137 evidence has no `bl`/`blr` in the
handler.

All 76 release tests and `reports/task137-aot-object-smoke.jsonl` pass. In the short
Earley residual probe, the selected shape fell from 273,574 entries to 1. The exact
alternating four-run full-suite comparison is
`reports/task137-aot-object-full-ab-4/comparison.txt`: 1181.66 → **1191.71** (+0.85%).
EarleyBoyer improves 1.84%, Splay 4.03%, RayTrace 2.11%, and every component clears the
standing floor. Accepted binary SHA-256:
`0fca2980f5fe4ac92ca6e87b1e0a8853c721cb07ba6f33426acb9b11cfdd4413`.
