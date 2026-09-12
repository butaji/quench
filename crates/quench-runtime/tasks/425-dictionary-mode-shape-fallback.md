# 425 — Dictionary-mode fallback for unbounded shapes

Status: planned

LuaJIT-remake's `Structure` never supports property deletion and caps shared shapes at
`x_maxNumSlots` (253) properties; past that, or on any operation a shared-shape tree cannot
represent, the object transitions once to `CacheableDictionary` — a private, per-object hash
table with its own linear-probing storage that allows arbitrary insertion and deletion with
no further shape-tree growth. This keeps the shared shape tree (Task 312, 150) permanently
bounded and cheap to transition, and moves the unbounded/mutable case to a structure that is
allowed to be slower per-object since it is, by construction, no longer shared or cacheable
by a monomorphic IC.

Quench's shape/shape-registry tasks (150, 312, 297 "shape-registry unbounded growth
validation") test and bound growth but none commit to an explicit terminal fallback
representation: what an object becomes once it legitimately cannot stay on the shared shape
tree (too many properties, or a delete Quench must support that Lua tables don't need to).
Task 297 validates the registry doesn't grow unboundedly; this task asks what receives the
object when a single instance's shape would have to.

Acceptance: a defined per-object dictionary representation reachable only via one explicit
transition from the shared shape tree; a test forcing the transition (either the max-slot
count or a delete) and proving the object keeps correct property semantics with no further
shape-tree growth attributable to it; existing monomorphic property-IC tests proving a
dictionary-mode object correctly de-optimizes those ICs rather than corrupting them; full
correctness suite.

Primary source: luajit-remake `runtime/structure.h`
<https://github.com/luajit-remake/luajit-remake/blob/master/runtime/structure.h>
(dictionary-mode transition and `CacheableDictionary`).
