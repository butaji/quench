# 426 — sqrt(D)-depth anchor blocks for shape property lookup

Status: planned

LuaJIT-remake's `Structure` stores properties in fixed-size blocks (`x_hiddenClassBlockSize`
= 16): the current non-full block (up to 15 properties) is scanned via a small inline hash
table living right next to the Structure header (negative-offset indexing so no separate
allocation is needed), while every full 16-property block is reachable through an "anchor"
built once every 2 blocks — a hash table containing every property from the shape-tree root
up to that anchor point. Property lookup is then: check the current block's inline table,
else jump to the nearest anchor and do one more hash lookup — O(1) amortized regardless of
total property count D, while insertion only pays for building a new anchor every ~16
properties, giving O(sqrt(D)) amortized insertion cost instead of O(D) for a naively rebuilt
full-history table.

Quench's shape tasks (150, 312, 154 "shape-set stub folding") represent the transition tree
and per-object property storage but grep found no task describing a bounded-lookup-depth
scheme for a shape with many properties: today, either every shape's full property list is
walked/hashed on lookup, or an unbounded structure is implied. This is a concrete, bounded
alternative worth costing against Quench's actual property-count distribution (most JS
objects have far fewer than 16 properties, so the win is specifically for wide objects and
polymorphic containers).

Acceptance: a lookup-cost benchmark comparing current per-shape property resolution against
the anchor-block scheme across a property-count sweep (1, 16, 64, 253+ properties);
correctness tests for lookup, insertion, and the anchor-rebuild boundary; full correctness
suite; adopt only if the wide-object case improves without regressing the common
few-property case measured in the V8v7 A/B gate.

Primary source: luajit-remake `runtime/structure.h`
<https://github.com/luajit-remake/luajit-remake/blob/master/runtime/structure.h>
(inline hash table, anchor hash table, hidden-class block scheme).
