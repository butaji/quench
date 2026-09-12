# 421 — Intrusive call-IC retarget list for tier transitions

Status: planned

When a Deegen-generated function tiers up, call sites that cached it are not found by
scanning the heap: each function keeps an intrusive circular doubly-linked list of the call-IC
stubs that target it, so tiering-up walks exactly the call sites that reference that function
and repatches each stub's jump target in place — cost proportional to the number of actual
call sites, not to program size.

Quench's tiering tasks (22, 45) and monomorphic/inherited call-IC tasks (140, 325, 336, 407)
describe caching and activation reuse but no indexed, owned structure for bulk-repatching
every caller of a function that changes native representation. Without one, tiering up (or any
future re-cooking of a stencil that existing call-ICs point at) either requires a heap/IC-table
scan or leaves stale targets. Add the intrusive list (or an equivalent owned index) as the one
canonical fact a function publishes about who currently calls it natively, and make tier
transitions and stencil recooking consume it instead of a scan.

Acceptance: a function-tier-up test with N monomorphic call sites showing repatch cost scales
with N, not with total call-IC count; no dangling stub after a function is re-cooked or freed;
full correctness suite; V8v7 A/B gate with no component-floor violation.

Primary source: sillycross, "Building a baseline JIT for Lua automatically"
<https://sillycross.github.io/2023/05/12/2023-05-12/> (Call IC / tiering-up section).
