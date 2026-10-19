# JIT design actions: Futamura, Ershov, and measured VM engineering

This is the execution list for the v2 JavaScript JIT. The static program is the
validated residual bytecode; runtime values, object layouts, prototype state,
and host effects are dynamic unless guarded. The first Futamura projection is
the practical target: specialize the VM's bytecode semantics for a hot
function, then let Cranelift optimize the resulting native graph. Ershov's
register-demand ordering applies to pure expression trees; CFG-wide allocation
belongs to Cranelift's SSA allocator. Neither idea excuses changing JavaScript
semantics or keeping code that fails the measured score and RSS gates.

Status key: `done` means already present or completed in this work; `active`
means implementation is underway; `queued` means it remains an explicit
engineering action. Completion requires code or recorded evidence, not merely
discussion.

## 1. Evidence and compiler contract

1. **Done** — Keep JetStream 3.0 as the acceptance corpus and stock validation
   as the semantic oracle; record score and maximum RSS together.
2. **Done** — Keep one frontend, residual bytecode family, `Vm`, heap,
   Cranelift ISA cache, W^X code allocator, and process-wide JIT admission
   budget for JavaScript and Wasm. Their semantic lowerers may differ, but
   duplicate JIT runtimes, caches, or execution paths are not acceptable.
3. **Queued** — Retain per-function diagnostics for promotions, compile time,
   code bytes, native entries, and interpreter fallback entries.
4. **Active** — Give every JS compile rejection a stable reason code and
   report hot rejected functions by time-weighted opportunity.
5. **Queued** — Attribute per-fixture time to bytecode op families, VM calls,
   allocations, GC, and native regions with low-overhead sampling.
6. **Queued** — Separate cold-start, warm-up, and steady-state score and RSS;
   include compiler scratch allocations in peak-memory accounting.
7. **Queued** — Use randomized paired rounds, pinned binaries, binary hashes,
   and predeclared acceptance intervals for every retained optimization.

## 2. Futamura-style specialization pipeline

8. **Queued** — Decode validated bytecode once into a compact JIT IR with
   explicit blocks, operands, constants, and source-PC mappings.
9. **Queued** — Build a CFG and reachability map before lowering; reject
   malformed or unsupported control flow before creating native code.
10. **Queued** — Add a binding-time lattice for static facts (opcodes,
    constants, local slots) and dynamic facts (JS values, shapes, prototypes).
11. **Queued** — Partially evaluate opcode selection, operand decoding, and
    constant loads while leaving dynamic JS semantics behind explicit guards.
12. **Queued** — Share opcode semantic descriptions between interpreter and
    JIT lowering so new op support cannot drift from the reference behavior.
13. **Queued** — Keep each lowering small and compositional: opcode effects
    map to Cranelift values, blocks, guards, and explicit slow-path edges.
14. **Queued** — Track abstract values through blocks: Number, Boolean,
    constant string, tagged unknown, and proven object-field facts.
15. **Queued** — Merge abstract states at joins with explicit phi nodes or
    reject the join with a diagnosable reason; never silently pick a path.
16. **Queued** — Use backward liveness to avoid carrying dead registers and
    locals into native blocks and resume maps.
17. **Queued** — Add bounded inlining for hot, small, same-program callees,
    guarded by call-target identity and code-size/payback limits.
18. **Queued** — Add on-stack replacement only after a bytecode-PC and
    register-state map can transfer an active interpreter frame exactly.

## 3. Ershov, SSA, and machine-code quality

19. **Queued** — Compute Sethi-Ullman/Ershov demand for pure expression trees
    and lower the higher-demand child first when dependencies permit.
20. **Done** — Keep tree scheduling local; pass CFG live ranges to
    Cranelift's allocator rather than inventing a second global allocator.
21. **Queued** — Fold constants and simplify branches in the JIT IR before
    Cranelift, preserving JS coercion and exception rules.
22. **Queued** — Eliminate redundant tag checks, conversions, and repeated
    guards when a dominating proof remains valid across the region.
23. **Queued** — Use native integer or floating-point forms only under explicit
    range, NaN, signed-zero, overflow, and coercion proofs.
24. **Done** — Keep Number fast paths separate from BigInt and mixed-type
    behavior; guard misses must execute the ordinary semantic path.
25. **Queued** — Hoist loop-invariant field and global guards only across
    regions that cannot mutate the relevant shape or prototype state.
26. **Queued** — Add machine-code disassembly capture for selected candidates,
    including code size, spills, branches, and safepoint locations.
27. **Queued** — Compare Cranelift opt levels and optimization passes on
    representative fixtures; keep a variant only after paired measurements.
28. **Done** — Keep ABI signatures explicit per artifact and validate the
    emitted entry shape before constructing executable memory.

## 4. Guards, effects, and deoptimization

29. **Queued** — Represent every speculative assumption as a guard dependency
    with a named invalidation source and fallback PC.
30. **Queued** — Centralize inline-cache guards for shape, data-property,
    prototype, and accessor status; do not duplicate partial cache logic.
31. **Queued** — Invalidate dependent code on shape/prototype mutation, or
    attach a version check at entry where invalidation would be too costly.
32. **Queued** — Preserve observable effect order for getters, coercions,
    allocations, host calls, exceptions, and writes.
33. **Queued** — Add effect summaries so pure regions can optimize freely and
    effectful operations call semantic VM helpers in original order.
34. **Queued** — Add precise root maps before generated code can allocate,
    call the VM, or reach a GC safepoint.
35. **Done** — Make interruption polls cycle-based and shared across all
    native backedges, with cancellation tests for long-running regions.
36. **Done** — Resume through the existing dispatcher with the same frame,
    registers, locals, handlers, and pending completion on all guard misses.
37. **Queued** — Differentially execute every supported opcode/region in
    interpreter and JIT modes across edge values and randomized inputs.

## 5. Tiering, memory, and single-VM runtime

38. **Done** — Keep the JIT opt-in and use the existing VM execution mode;
    no frontend-specific VM or duplicate runtime is permitted.
39. **Queued** — Promote by expected payback: estimated future calls times
    measured per-call savings must exceed compile cost and code-memory charge.
40. **Queued** — Track call and loop hotness separately so one long-running
    loop can tier up without waiting for many function entries.
41. **Queued** — Bound all JIT metadata, temporary IR, executable mappings,
    and code-cache pages with one process-wide budget.
42. **Queued** — Evict cold code by estimated remaining value and retain
    counters only for functions with demonstrated hotness.
43. **Queued** — Reuse immutable Cranelift settings/ISA and compiler scratch
    storage across compilations where thread-safety permits.
44. **Queued** — Avoid per-call heap allocations and copies at JIT entry;
    use inline argument buffers and borrowed VM strings with scoped lifetimes.
45. **Queued** — Keep executable mappings W^X, page accounting exact, and
    code retirement safe with respect to active native frames.

## 6. Workload coverage and acceptance

46. **Queued** — Rank candidates using native-entry frequency and fixture
    contribution, then target hot rejected loops before cold helper methods.
47. **Queued** — Implement general string, object, and array operations behind
    guards where JetStream profiles show repeatable dominance.
48. **Queued** — Measure allocation rate, retained heap, GC pauses, mapped code,
    and peak RSS together; reject speedups that break the memory objective.
49. **Queued** — Run the complete JetStream 3 suite, including Wasm-tagged
    fixtures, after any tiering or shared-dispatch change.
50. **Queued** — Claim victory only after 11+ matched rounds show Quench's
    paired score ratio above 1 and peak-RSS ratio below 1 against each
    available JIT-enabled runtime on the same supported host.

## Execution order

The current completed set is **1, 2, 20, 24, 28, 35, 36, and 38**. Action 4
is active; the other actions remain queued until code or evidence closes them.

Action 2 is complete: both language lowerers request the same cached Cranelift
ISA, both publish through the same W^X `CodeMemory`, and JS/Wasm artifacts
consume one shared process-wide code-cache budget. Their frontends lower
different validated bytecode semantics. A regression test compiles and enters
both tiers through the same `Vm`. Action 4 is active: compile failures now
carry stable rejection categories in opt-in diagnostics; hot functions that
never reach a lowering attempt still need to be surfaced by opportunity. Next,
build actions 8–18 as one general bytecode specializer, extend guarded machine
lowering in actions 19–37, and tune tiering and memory admission in actions
39–45. The current underscore-helper specialization is a narrow test of
native string access and shared-VM return behavior; it is not a substitute for
general string lowering. Each batch must retain differential checks and
JetStream evidence before its status changes to `done`.
