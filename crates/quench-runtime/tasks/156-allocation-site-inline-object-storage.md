# 156 — Allocation-site inline object storage

Status: planned

Give ordinary objects a small fixed in-object slot area selected from immutable
allocation-layout kernels. Object literals and constructor sites know an initial shape
and required slot count, so their allocation stencil reserves and initializes one cell;
only overflow properties allocate an out-of-line backing store. A shape descriptor says
whether each slot is inline or out-of-line, preserving the existing fixed-offset IC
interface.

Implement this on Task 148's VM heap rather than adding more `Rc` ownership. Layout
classes and inline capacities are named constants derived from measured object-size
histograms. No V8v7 property names or source locations choose a layout; the choice uses
only allocation semantics and required property count.

Acceptance: common object literals require one bump allocation, have fixed inline slot
offsets, and perform no `Vec` allocation; growth/deletion/prototype semantics remain
correct; allocation and pointer-load counts fall on Richards, DeltaBlue, and Splay; full
V8v7 A/B passes with resident-memory reporting.

Primary sources: <https://v8.dev/blog/slack-tracking> and
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>.

