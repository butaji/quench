# 320 — Sticky-mark generational object collection

Status: planned

Extend the stable-cell object heap with a non-moving young/old generation state. New
cells are young. A young collection traces roots plus a remembered set, reclaims only
unmarked young cells, and promotes survivors by leaving their mark state sticky. A full
collection resets the logical old mark epoch and traces all reachable cells. Object
addresses never move, preserving shape/property IC and raw-handle validity.

Keep the first implementation single-threaded and stop-the-world. Concurrent marking,
copying evacuation, pretenuring feedback, and compaction are out of scope. Store age and
mark state in named bit fields or enums; use named young/full budgets and promotion
ages. A collection kind is explicit data selected by allocation budgets, never a hidden
branch inside the tracer.

The remembered set must be installed before enabling young-only reclamation. Every
heap pointer store flows through the canonical write effect so [[321]] can derive its
barrier and its verified elisions. Roots include guest activations, side exits, globals,
environments, function captures, IC-owned heap references, and host root scopes.

Acceptance: adversarial old-to-young, cycles, prototype, arrays, closures, exceptions,
IC roots, and full/young alternation tests pass under stress; young collections do not
scan unrelated old objects; stable handles remain unchanged; Splay allocation/GC
counters and complete-suite alternating A/B improve before enabling by default.

Primary sources:
<https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>,
<https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/>,
and <https://v8.dev/blog/trash-talk>.

