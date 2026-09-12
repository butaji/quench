# 423 — Butterfly-style split object storage

Status: planned

LuaJIT-remake's table objects use a single allocation shared by two storage regions that
grow in opposite directions from one pointer: the array/vector part grows upward at
positive offsets from the `Butterfly*` (`((TValue*)butterflyPtr)[index]`, with the header
occupying exactly one `TValue` slot so array indexing needs no extra offset math), and
named-property storage grows downward at negative offsets from the same pointer. One
allocation serves both the dense-index fast path and the hidden-class-indexed named
property path, with no separate header/indirection per part and no second allocation to
keep resident when an object holds both.

Quench's object model (Tasks 09, 137, 156, 165, 369) already separates array and named
storage conceptually and has typed/split payload kinds (369), but grep of the task set found
no task describing a single-allocation, opposite-direction-growth layout — objects that mix
dense elements and named properties currently imply two allocations or two headers. Evaluate
whether collapsing to one allocation (one pointer, header at the pivot, positive indices for
elements, negative indices for named slots) reduces allocation count and improves locality
for the common "array-like object with a few named properties" shape, without complicating
resize/rehash (growing the array part still requires reallocation and copy on both sides, as
in the source).

Acceptance: a representation proposal reusing the existing hidden-class/shape machinery for
the negative-offset side; a benchmark isolating objects with both array and named data,
measuring allocation count and cache-miss/locality change versus the current two-allocation
layout; full correctness suite; V8v7 A/B gate with no component-floor violation before
acceptance.

Primary source: luajit-remake `runtime/butterfly.h`
<https://github.com/luajit-remake/luajit-remake/blob/master/runtime/butterfly.h>; JSC's
original Butterfly design, which this is modeled on.
