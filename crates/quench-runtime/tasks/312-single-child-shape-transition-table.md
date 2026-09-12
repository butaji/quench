# 312 — Single-child shape-transition table

Status: planned

Replace `ShapeRegistry`'s unconditional per-parent inner transition `HashMap` with a
small sum:

`Empty | One { atom, child } | Many(Map<AtomId, ShapeRef>)`.

The common constructor/object-literal path grows a linear hidden-class chain and should
remain in `One`; only a genuinely branching parent promotes once to `Many`. Addition and
removal edges use the same representation or two explicitly named tables if their
semantics require separation. Property names should become interned `AtomId`s at this
edge so the transition probe never allocates or hashes string contents after
internalization.

This is a representation of the already-canonical transition fact from Task 150, not a
second shape registry. Shapes remain immutable shared kernels; objects still contain one
shape reference and fixed values. Use named constants for inline capacity and promotion
policy; do not encode tagged-pointer bits as literals.

Acceptance: linear construction creates no per-node hash map; the second distinct child
promotes exactly once; repeated addition/removal returns the canonical child; property
order and IC invalidation remain correct; allocation counters confirm the intended cost
disappeared; full release/stress tests and alternating V8v7 A/B decide retention.

Primary source: JavaScriptCore `StructureTransitionTable`, whose common single transition
is stored directly and promoted to a map on branching:
<https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/StructureTransitionTable.h>.

Depends on Tasks 07, 11, 135, 148, and 150.
