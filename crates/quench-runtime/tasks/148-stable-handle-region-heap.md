# 148 — Stable-handle region heap

Status: in_progress

Replace hot `Rc` retain/release and `RefCell` traffic with a VM-owned, non-moving heap.
`Value` keeps its existing word-sized tag and carries a stable heap handle/pointer. The
initial collector is deliberately small: size-classed bump/free-list allocation plus a
precise stop-the-world mark/sweep over VM frames, registers, environments, constants,
closures, IC owners, and host roots. Non-moving cells preserve the pointer assumptions
already used by shape guards and linked stencils.

This is an ownership representation change, not an invitation for raw untracked
pointers. Centralize allocation, root enumeration, tracing, finalization, weak edges,
and collection safepoints. Use named constants for region size, size classes, mark bits,
and collection thresholds. Keep slow collection as a shared kernel; ordinary value copy,
slot load/store, and property hits must perform no atomic or non-atomic reference-count
updates.

The design integrates Tasks 09 and 38. Immix supplies the region/line allocation model
(<https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>); JavaScriptCore's GC
description supplies relevant production constraints for cells and generational tracing
(<https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>). Generational
and concurrent collection are later derived refinements, not MVP requirements.

Acceptance: cycles are reclaimed; closure/object/string/RegExp/native-resource lifetime
tests pass; a precise-root stress mode collects at every safe point; profiles show no
`Rc` retain/release or `RefCell` borrow operations in hot object/call paths; complete
V8v7 A/B improves and resident memory remains bounded.

## First implementation slice: object-only cutover

Migrate the complete `OBJECT_TAG` domain at once to a stable `ObjectHandle`; mixing an
`Rc<ObjectCell>` and a heap handle under one tag is forbidden. Centralize every object
creation site in `Vm::allocate_object`, keep property slots out of line initially, and
preserve the Task 137 shape/slot ABI. Object-valued local/register moves then become plain
word copies while strings, functions, and RegExp values may retain their current ownership
temporarily.

Use a single-threaded linear allocation area: the fast path is a named alignment operation,
one cursor-limit comparison, and cursor bump; exhaustion enters the collector kernel. The
first collector is precise, stop-the-world, and non-moving. Its root set is the active
guest-frame chain, environments, globals, constants, IC-owned values, and explicit host
temporary scopes. Stress mode collects at every canonical allocation safepoint.

Do not begin with a nursery, concurrent marking, or compaction. V8's allocator confirms
the useful mutator primitive is `top + size <= limit` followed by increment; JSC's block
directory and logical versions remain later refinements owned by Task 162. After this
cutover, Task 156 can add inline property slots and Task 191 can group dominated bumps.

Additional primary sources: V8's linear allocation fast path
<https://chromium.googlesource.com/v8/v8.git/+/refs/heads/13.1.57/src/heap/main-allocator-inl.h>
and V8's generational collector overview <https://v8.dev/blog/trash-talk>.

## 2026-09-10 implementation checkpoint

The object-only handle cutover is present as an experimental source slice: the object
tag now carries `ObjectHandle`, allocation is centralized in the VM heap, and ordinary
object-valued register/local transfers no longer clone `Rc<ObjectCell>`. This is not yet
an accepted implementation. At the initial cutover checkpoint, the release suite passed
100 of 101 tests; `numeric_property_view_rejects_unsafe_categories` observed one extra
strong owner retained by the test heap. The later ownership-predicate checkpoint below
supersedes that test result, but precise object-lifetime tests and a complete alternating
V8v7 A/B remain required before marking this task complete.

## 2026-09-10 rejected ownership-predicate experiments

Two attempts to let object handles bypass the remaining `Rc` ownership guard were measured
and removed. Extending all cooked move/load/store/return handlers from the original single
heap-tag threshold to a closed `STRING_TAG | FUNCTION_TAG | REGEXP_TAG` predicate enlarged
the cooked catalog from 12,172 to 13,148 bytes and measured 2,153.37 to 2,106.82
(-2.16%) in `reports/task148-ownership-predicate-full-ab-5`. A compact form still produced
12,776 bytes. Restricting the change to runtime `Value::overwrite` preserved the original
catalog but measured 2,187.83 to 2,150.87 (-1.69%) in
`reports/task148-runtime-ownership-full-ab-5`, with seven of eight suites negative.

Both variants were reverted. The AOT handlers again use their original single heap-tag
threshold and runtime overwrite again uses `is_heap()`. Object handles still remove
`Rc<ObjectCell>` from the representation itself; the rejected experiments show that a
broader non-contiguous ownership predicate costs more than it saves in these tiny transfer
paths. Future ownership removal must arrive through a coarser typed region where the guard
is proved once, not through another per-transfer predicate.

## 2026-09-10 completed-frame reclamation checkpoint

The object heap now has a single explicit cell-state machine (`Allocated`, `Marked`,
`Free`) stored beside non-moving chunk cells. Allocation reuses the free list before the
linear bump area and only sets a collection-request bit after a named allocation threshold;
it never sweeps while a native helper may hold an unregistered temporary or object borrow.
The request is serviced by the slow collector kernel only from `complete_dyn_frame`, while
the completed frame's result or thrown value and every suspended parent frame remain linked
in the VM's intrusive active-frame chain.

Root tracing is iterative and precise over globals, the array prototype, builtin values,
active frame-owned slots/results/throws/numeric-region owners, environments, functions,
objects, and call-IC-owned callees/environments. Object edges include out-of-line property
slots, dense array elements, and prototypes. Function and environment identities are
deduplicated so closure cycles cannot recurse indefinitely. Reclaimed cells are dropped
once and returned to stable-address free-list storage. Collection also advances the
prototype epoch and clears every active or cached inherited-property/`instanceof` identity
guard before an address can be reused, preventing an ABA cache hit.

Four focused tests cover transitive property/array/prototype roots, cyclic reclamation and
address reuse, nested return/throw roots across child and parent completion, and bounded
live-cell counts across repeated stencil calls. `cargo test --release` passes 105 of 105,
and the same 105 tests pass with `DEEGEN_OBJECT_GC_STRESS=1`, which requests collection at
every completed dynamic frame. Per instruction, no V8v7 benchmark was run for this
checkpoint.

Task 148 remains in progress: migration of strings, functions, and RegExp state off
transitional `Rc` ownership is still required, as are the remaining lifetime tests and the
V8v7 performance acceptance named above. Explicit host root scopes are now implemented by
Task 305. Generational/concurrent collection,
inline object slots, and grouped allocation remain owned by Tasks 162, 156, and 191.

## 2026-09-10 O(1) cell-local marking experiment

A first smoke run of the completed-frame collector exposed that `ObjectHeap::mark` scanned
the chunk list for every traced object edge. That made marking proportional to live edges
times chunk count and overwhelmed Splay despite bounded memory. The experimental follow-up
keeps the same one-word `ObjectHandle` and offset-zero object payload ABI, but moves heap
identity and the `Allocated`/`Marked`/`Free` state into the permanently initialized tail of
each `ObjectCell`. Marking is now one cell dereference, one heap-identity comparison, and one
state transition; it does not search the chunk list.

The payload is represented as `UnsafeCell<MaybeUninit<Object>>`. Sweep drops only that
payload and leaves the header initialized as `Free`; reuse writes a new payload and changes
the state back to `Allocated`. Consequently free-list entries need only the cell pointer,
and the duplicate per-chunk state array, chunk pointer, and cell index are gone. A named,
monotonic heap identity distinguishes VM-owned cells from externally pinned test cells
without enlarging `ObjectHandle`.

The focused heap-scope test verifies constant-shape local marking and non-mutation of a
foreign heap cell. Both `cargo test --release` and
`DEEGEN_OBJECT_GC_STRESS=1 cargo test --release` pass 106 of 106 tests. This optimization
remains experimental until a separate frozen-binary V8v7/RSS comparison confirms that it
removes the marking regression; no benchmark was run as part of this implementation turn.

## 2026-09-10 adaptive collection-debt experiment

The O(1) marker removed chunk lookup from each edge, but a fixed one-chunk allocation
threshold still forced repeated full scans of a large stable live graph. The trigger is now
an explicit dynamic allocation budget. It starts at one object chunk; after every sweep it
becomes the greater of one chunk and `live_cells * OBJECT_LIVE_HEAP_GROWTH_FACTOR`, using
saturating multiplication. Allocation requests collection only after consuming that budget.
The `DEEGEN_OBJECT_GC_STRESS` override remains independent and still collects at every
completed dynamic frame.

A focused test marks a live set larger than one chunk, verifies the derived budget, verifies
that another chunk of allocation does not request a redundant scan, and verifies that the
request appears exactly when the larger budget is consumed. Both `cargo test --release` and
the every-frame stress suite passed 107 of 107 tests at this checkpoint.

The frozen adaptive binary
`52d63739be092be2d9920af854ed21f3363def112cbafb5d4d0eb1625b46ea58`
was then compared with the pre-collection object-handle binary over five alternating
500 ms pairs. Aggregate V8v7 fell from 2,155.64 to 2,068.59 (-4.04%); RegExp was the
largest loss at -12.10%, while Navier-Stokes was +0.33%. A two-second Splay run reported
443,072,512 bytes maximum resident set size and score 3,669. Evidence is in
`reports/task148-adaptive-gc-full-ab-5`.

The adaptive policy therefore fails the performance gate but replaces the earlier
multi-gigabyte transient growth with a bounded experimental baseline. It remains in source
while Task 148 is in progress. The evidence specifically rejects another debt-multiplier
tweak: a long-running frame needs a canonical in-frame allocation safepoint before the
collector policy can be accepted.

## 2026-09-10 explicit host-root boundary

Task 299's bump-first allocator stopped immediately overwriting reclaimed cells and exposed
a latent ownership error: a Rust-held function retained across a later VM entry was not in
the collector graph, so its prototype could be reclaimed. Task 305 adds one explicit
stack-disciplined `host_roots` sequence to `Vm`; collection traces it, and scope release
truncates it to its captured base. Internal `Value` moves remain non-owning one-word copies.

The constructor regression and a focused retain/collect/release lifecycle test pass with
collection requested at every completed frame. This closes the object-only host-root gap;
future embedding APIs must return an explicit persistent/scoped wrapper instead of exposing
raw `Value` as an owning handle.

## Round-twenty allocator and sweep refinements

Primary-source research identified two bounded follow-ups without changing the heap's
semantic model. Task 299 first makes the untouched active-chunk tail the literal bump fast
path; the current implementation unnecessarily borrows and probes the recycled-cell vector
before that path. Only after its frozen A/B should Task 300 be considered: block-local mark
bitmaps, logical mark epochs, and allocation-driven lazy sweeping can avoid eager whole-heap
metadata work while retaining one-word non-moving handles.

These are ordered experiments, not accumulated mechanisms. Task 299 refines allocation in
the current heap. Task 300 replaces the mark/sweep metadata representation only if Task 148
GC counters show sweep work remains material. A sticky non-moving nursery remains Task 162
and follows only if repeated traversal of a large old live graph is the dominant cost.

## Task 359 ownership evidence

The object-only cutover is now accepted as a useful but incomplete representation step:
Task 358 corrected the cooked transfer predicate and improved the full aggregate 0.30%.
Task 359 then selected a coarse property-store terminal but sent all 1,280,750 targeted
entries to the generic executor because strings/functions/regexps still carry `Rc`
ownership. This directly raises the priority of finishing the one-heap cutover.

Do not add another heap representation beside `ObjectHeap`. Generalize its stable cell to
the canonical heap-cell coproduct and migrate one complete tag domain at a time, including
its roots and finalization, without mixing an `Rc` and heap handle under one tag. Captured
environments must join the traced graph before function cells can safely stop owning them.
Task 172's explicit `StoreTake` is the bounded bridge while this migration proceeds;
Tasks 193/320/321 supply precise roots, the nursery, and the write barrier after the cell
domains are unified.
